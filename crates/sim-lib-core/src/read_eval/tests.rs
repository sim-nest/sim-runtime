use std::sync::Arc;

use sim_codec_lisp::LispCodecLib;
use sim_kernel::{
    CapabilityName, CapabilitySet, Cx, DefaultFactory, Demand, EagerPolicy, Error, EvalPolicy,
    Expr, PreparedArgs, RawArgs, TrustLevel, read_construct_capability, read_eval_capability,
};
use sim_shape::{AnyShape, ExprKind, ExprKindShape};

use super::*;

fn origin() -> RequestOrigin {
    RequestOrigin::new(Symbol::qualified("test", "origin"))
}

fn codec() -> Symbol {
    Symbol::qualified("codec", "lisp")
}

fn policy(trust: TrustLevel, capabilities: CapabilitySet) -> ReadPolicy {
    ReadPolicy {
        trust,
        capabilities,
    }
}

fn trusted_read_eval_policy() -> ReadPolicy {
    policy(
        TrustLevel::TrustedSource,
        CapabilitySet::new().grant(read_eval_capability()),
    )
}

fn request_with(source: ReadEvalSource, expected_shape: Arc<dyn Shape>) -> ReadEvalRequest {
    ReadEvalRequest {
        origin: origin(),
        codec: codec(),
        source,
        read_policy: trusted_read_eval_policy(),
        requires: Vec::new(),
        allow: CapabilitySet::new().grant(read_eval_capability()),
        expected_shape,
    }
}

struct ActiveCapabilityPolicy {
    capability: CapabilityName,
}

impl EvalPolicy for ActiveCapabilityPolicy {
    fn name(&self) -> &'static str {
        "active-capability-probe"
    }

    fn prepare_call_args(
        &self,
        cx: &mut Cx,
        raw: RawArgs,
        demands: &[Demand],
    ) -> Result<PreparedArgs> {
        EagerPolicy.prepare_call_args(cx, raw, demands)
    }

    fn force(&self, cx: &mut Cx, value: Value, demand: Demand) -> Result<Value> {
        EagerPolicy.force(cx, value, demand)
    }

    fn eval_expr(&self, cx: &mut Cx, _expr: Expr) -> Result<Value> {
        cx.factory()
            .bool(cx.capabilities().contains(&self.capability))
    }
}

fn probe_cx(capability: CapabilityName) -> (Cx, sim_kernel::GrantSeat) {
    Cx::new_seated(
        Arc::new(ActiveCapabilityPolicy { capability }),
        Arc::new(DefaultFactory),
    )
}

#[test]
fn install_registers_broker_value() {
    let (mut cx, _seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));

    assert!(install_read_eval_broker(&mut cx).unwrap());
    assert!(!install_read_eval_broker(&mut cx).unwrap());

    let value = cx.resolve_value(&read_eval_broker_symbol()).unwrap();
    assert!(value.object().downcast_ref::<ReadEvalBroker>().is_some());
}

#[test]
fn missing_read_eval_capability_is_denied() {
    let (mut cx, seat) = probe_cx(read_eval_capability());
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    let mut request = request_with(
        ReadEvalSource::Expr(Expr::Nil),
        Arc::new(ExprKindShape::new(ExprKind::Bool)),
    );
    request.read_policy = policy(TrustLevel::TrustedSource, CapabilitySet::new());

    let err = ReadEvalBroker::new().admit(&mut cx, request).unwrap_err();

    assert!(matches!(
        err,
        Error::CapabilityDenied { capability } if capability == read_eval_capability()
    ));
}

#[test]
fn untrusted_read_eval_policy_is_denied() {
    let (mut cx, seat) = probe_cx(read_eval_capability());
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    let mut request = request_with(
        ReadEvalSource::Expr(Expr::Nil),
        Arc::new(ExprKindShape::new(ExprKind::Bool)),
    );
    request.read_policy = policy(
        TrustLevel::Untrusted,
        CapabilitySet::new().grant(read_eval_capability()),
    );

    let err = ReadEvalBroker::new().admit(&mut cx, request).unwrap_err();

    assert!(matches!(
        err,
        Error::TrustDenied { capability, trust }
            if capability == read_eval_capability() && trust == TrustLevel::Untrusted
    ));
}

#[test]
fn required_capability_must_be_held_by_caller() {
    let required = CapabilityName::new("test.required");
    let (mut cx, seat) = probe_cx(read_eval_capability());
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    let mut request = request_with(
        ReadEvalSource::Expr(Expr::Nil),
        Arc::new(ExprKindShape::new(ExprKind::Bool)),
    );
    request.requires = vec![required.clone()];

    let err = ReadEvalBroker::new().admit(&mut cx, request).unwrap_err();

    assert!(matches!(
        err,
        Error::CapabilityDenied { capability } if capability == required
    ));
}

#[test]
fn allowed_capability_absent_from_caller_is_not_active() {
    let extra = CapabilityName::new("test.extra");
    let (mut cx, seat) = probe_cx(extra.clone());
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    let mut request = request_with(
        ReadEvalSource::Expr(Expr::Nil),
        Arc::new(ExprKindShape::new(ExprKind::Bool)),
    );
    request.allow = CapabilitySet::new().grant(extra);

    let value = ReadEvalBroker::new().admit(&mut cx, request).unwrap();

    assert_eq!(value.object().as_expr(&mut cx).unwrap(), Expr::Bool(false));
    assert!(cx.capabilities().contains(&read_eval_capability()));
}

#[test]
fn shape_mismatch_is_denied() {
    let (mut cx, seat) = probe_cx(read_eval_capability());
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    let request = request_with(
        ReadEvalSource::Expr(Expr::Nil),
        Arc::new(ExprKindShape::new(ExprKind::String)),
    );

    let err = ReadEvalBroker::new().admit(&mut cx, request).unwrap_err();

    assert!(matches!(err, Error::WrongShape { .. }));
}

#[test]
fn happy_path_returns_value_and_restores_caller_capabilities() {
    let (mut cx, seat) = probe_cx(read_eval_capability());
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    expect_granted!(seat.grant(&mut cx, read_construct_capability()));
    let request = request_with(
        ReadEvalSource::Expr(Expr::Nil),
        Arc::new(ExprKindShape::new(ExprKind::Bool)),
    );

    let value = ReadEvalBroker::new().admit(&mut cx, request).unwrap();

    assert_eq!(value.object().as_expr(&mut cx).unwrap(), Expr::Bool(true));
    assert!(cx.capabilities().contains(&read_eval_capability()));
    assert!(cx.capabilities().contains(&read_construct_capability()));
}

#[test]
fn text_source_decodes_through_named_codec() {
    let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    let lib = LispCodecLib::new(sim_kernel::CodecId(1)).unwrap();
    cx.load_lib(&lib).unwrap();
    let request = request_with(
        ReadEvalSource::Text("\"ok\"".to_owned()),
        Arc::new(ExprKindShape::new(ExprKind::String)),
    );

    let value = ReadEvalBroker::new().admit(&mut cx, request).unwrap();

    assert_eq!(
        value.object().as_expr(&mut cx).unwrap(),
        Expr::String("ok".to_owned())
    );
}

#[test]
fn bytes_source_decodes_through_named_codec() {
    let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    expect_granted!(seat.grant(&mut cx, read_eval_capability()));
    let lib = LispCodecLib::new(sim_kernel::CodecId(1)).unwrap();
    cx.load_lib(&lib).unwrap();
    let request = request_with(ReadEvalSource::Bytes(b"nil".to_vec()), Arc::new(AnyShape));

    let value = ReadEvalBroker::new().admit(&mut cx, request).unwrap();

    assert_eq!(value.object().as_expr(&mut cx).unwrap(), Expr::Nil);
}
