//! Explicit read-eval admission through one diminished gate.

use std::sync::Arc;

use sim_codec::{Input, decode_with_codec};
use sim_kernel::{
    AbiVersion, CapabilityName, CapabilitySet, Cx, Diagnostic, Error, Export, Expr, Lib,
    LibManifest, LibTarget, Linker, LoadCx, Object, ReadPolicy, Result, Shape, ShapeId, Symbol,
    Value, Version, read_eval_capability,
};
use sim_shape::expected_shape_diagnostic;

/// Open origin data for an explicit read-eval request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestOrigin {
    /// Open tag for the request origin, such as `config/node` or `repl`.
    pub tag: Symbol,
    /// Optional origin detail carried as data for callers and ledger phases.
    pub detail: Option<Expr>,
}

impl RequestOrigin {
    /// Builds origin data from a tag with no detail.
    pub fn new(tag: Symbol) -> Self {
        Self { tag, detail: None }
    }

    /// Builds origin data from a tag and detail expression.
    pub fn with_detail(tag: Symbol, detail: Expr) -> Self {
        Self {
            tag,
            detail: Some(detail),
        }
    }
}

/// Source accepted by the read-eval broker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadEvalSource {
    /// Decode this text through the request codec before evaluation.
    Text(String),
    /// Decode these bytes through the request codec before evaluation.
    Bytes(Vec<u8>),
    /// Evaluate an already-decoded expression.
    Expr(Expr),
}

/// A single explicit, host-authorized read-eval admission request.
pub struct ReadEvalRequest {
    /// Open origin data describing who asked for eval.
    pub origin: RequestOrigin,
    /// Codec symbol used to decode text or bytes sources.
    pub codec: Symbol,
    /// Source to decode and evaluate, or an already-decoded expression.
    pub source: ReadEvalSource,
    /// Trusted host-built read policy; never derive this from request text.
    pub read_policy: ReadPolicy,
    /// Capabilities the caller must already hold before eval can run.
    pub requires: Vec<CapabilityName>,
    /// Maximum powers the request allows the eval body to run with.
    pub allow: CapabilitySet,
    /// Shape the evaluated result must satisfy before it is admitted.
    pub expected_shape: Arc<dyn Shape>,
}

// sim-non-citizen(reason = "host admission gate object; explicit request data is not a read-constructor surface", kind = "runtime", descriptor = "")
/// The one runtime admission gate for explicit diminished read-eval.
#[derive(Clone, Default)]
pub struct ReadEvalBroker;

impl ReadEvalBroker {
    /// Admits one explicit read-eval request or fails closed.
    pub fn admit(&self, cx: &mut Cx, request: ReadEvalRequest) -> Result<Value> {
        let ReadEvalRequest {
            origin: _origin,
            codec,
            source,
            read_policy,
            requires,
            allow,
            expected_shape,
        } = request;

        read_policy.require(&read_eval_capability())?;
        cx.require_all(&requires)?;

        let active = diminish_capabilities(cx.capabilities(), &allow);
        let value = cx.with_capabilities(active, |cx| {
            let expr = decode_source(cx, &codec, source, read_policy.clone())?;
            cx.eval_expr(expr)
        })?;

        let matched = expected_shape.check_value(cx, value.clone())?;
        if matched.accepted {
            return Ok(value);
        }

        Err(Error::WrongShape {
            expected: expected_shape.id().unwrap_or(ShapeId(0)),
            diagnostics: shape_diagnostics(cx, expected_shape.as_ref(), matched.diagnostics)?,
        })
    }
}

impl Object for ReadEvalBroker {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<read-eval-broker>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for ReadEvalBroker {
    fn class(&self, cx: &mut Cx) -> Result<sim_kernel::ClassRef> {
        cx.factory().class_stub(
            sim_kernel::ClassId(0),
            Symbol::qualified("read-eval", "Broker"),
        )
    }
}

/// Returns the broker value symbol exported by [`ReadEvalBrokerLib`].
pub fn read_eval_broker_symbol() -> Symbol {
    Symbol::qualified("read-eval", "broker")
}

/// Returns the manifest id for the read-eval broker library.
pub fn read_eval_broker_lib_id() -> Symbol {
    Symbol::qualified("sim", "read-eval-broker")
}

/// Loadable library that registers the read-eval broker value.
pub struct ReadEvalBrokerLib;

impl Lib for ReadEvalBrokerLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: read_eval_broker_lib_id(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Value {
                symbol: read_eval_broker_symbol(),
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.value(
            read_eval_broker_symbol(),
            cx.factory().opaque(Arc::new(ReadEvalBroker))?,
        )?;
        Ok(())
    }
}

/// Installs the read-eval broker library if it is not already loaded.
pub fn install_read_eval_broker(cx: &mut Cx) -> Result<bool> {
    crate::install_once(cx, &ReadEvalBrokerLib)
}

fn decode_source(
    cx: &mut Cx,
    codec: &Symbol,
    source: ReadEvalSource,
    read_policy: ReadPolicy,
) -> Result<Expr> {
    match source {
        ReadEvalSource::Text(text) => decode_with_codec(cx, codec, Input::Text(text), read_policy),
        ReadEvalSource::Bytes(bytes) => {
            decode_with_codec(cx, codec, Input::Bytes(bytes), read_policy)
        }
        ReadEvalSource::Expr(expr) => Ok(expr),
    }
}

fn diminish_capabilities(current: &CapabilitySet, allowed: &CapabilitySet) -> CapabilitySet {
    current
        .iter()
        .filter(|capability| allowed.contains(capability))
        .cloned()
        .fold(CapabilitySet::new(), CapabilitySet::grant)
}

fn shape_diagnostics(
    cx: &mut Cx,
    shape: &dyn Shape,
    diagnostics: Vec<Diagnostic>,
) -> Result<Vec<Diagnostic>> {
    if !diagnostics.is_empty() {
        return Ok(diagnostics);
    }
    let expected = match shape.symbol() {
        Some(symbol) => symbol.to_string(),
        None => shape.describe(cx)?.name,
    };
    Ok(vec![expected_shape_diagnostic(
        expected,
        "read-eval result",
    )])
}

#[cfg(test)]
mod tests {
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
        seat.grant(&mut cx, read_eval_capability());
        let mut request = request_with(
            ReadEvalSource::Expr(Expr::Nil),
            Arc::new(ExprKindShape::new(ExprKind::Bool)),
        );
        request.read_policy = policy(TrustLevel::TrustedSource, CapabilitySet::new());

        let err = ReadEvalBroker.admit(&mut cx, request).unwrap_err();

        assert!(matches!(
            err,
            Error::CapabilityDenied { capability } if capability == read_eval_capability()
        ));
    }

    #[test]
    fn untrusted_read_eval_policy_is_denied() {
        let (mut cx, seat) = probe_cx(read_eval_capability());
        seat.grant(&mut cx, read_eval_capability());
        let mut request = request_with(
            ReadEvalSource::Expr(Expr::Nil),
            Arc::new(ExprKindShape::new(ExprKind::Bool)),
        );
        request.read_policy = policy(
            TrustLevel::Untrusted,
            CapabilitySet::new().grant(read_eval_capability()),
        );

        let err = ReadEvalBroker.admit(&mut cx, request).unwrap_err();

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
        seat.grant(&mut cx, read_eval_capability());
        let mut request = request_with(
            ReadEvalSource::Expr(Expr::Nil),
            Arc::new(ExprKindShape::new(ExprKind::Bool)),
        );
        request.requires = vec![required.clone()];

        let err = ReadEvalBroker.admit(&mut cx, request).unwrap_err();

        assert!(matches!(
            err,
            Error::CapabilityDenied { capability } if capability == required
        ));
    }

    #[test]
    fn allowed_capability_absent_from_caller_is_not_active() {
        let extra = CapabilityName::new("test.extra");
        let (mut cx, seat) = probe_cx(extra.clone());
        seat.grant(&mut cx, read_eval_capability());
        let mut request = request_with(
            ReadEvalSource::Expr(Expr::Nil),
            Arc::new(ExprKindShape::new(ExprKind::Bool)),
        );
        request.allow = CapabilitySet::new().grant(extra);

        let value = ReadEvalBroker.admit(&mut cx, request).unwrap();

        assert_eq!(value.object().as_expr(&mut cx).unwrap(), Expr::Bool(false));
        assert!(cx.capabilities().contains(&read_eval_capability()));
    }

    #[test]
    fn shape_mismatch_is_denied() {
        let (mut cx, seat) = probe_cx(read_eval_capability());
        seat.grant(&mut cx, read_eval_capability());
        let request = request_with(
            ReadEvalSource::Expr(Expr::Nil),
            Arc::new(ExprKindShape::new(ExprKind::String)),
        );

        let err = ReadEvalBroker.admit(&mut cx, request).unwrap_err();

        assert!(matches!(err, Error::WrongShape { .. }));
    }

    #[test]
    fn happy_path_returns_value_and_restores_caller_capabilities() {
        let (mut cx, seat) = probe_cx(read_eval_capability());
        seat.grant(&mut cx, read_eval_capability());
        seat.grant(&mut cx, read_construct_capability());
        let request = request_with(
            ReadEvalSource::Expr(Expr::Nil),
            Arc::new(ExprKindShape::new(ExprKind::Bool)),
        );

        let value = ReadEvalBroker.admit(&mut cx, request).unwrap();

        assert_eq!(value.object().as_expr(&mut cx).unwrap(), Expr::Bool(true));
        assert!(cx.capabilities().contains(&read_eval_capability()));
        assert!(cx.capabilities().contains(&read_construct_capability()));
    }

    #[test]
    fn text_source_decodes_through_named_codec() {
        let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
        seat.grant(&mut cx, read_eval_capability());
        let lib = LispCodecLib::new(sim_kernel::CodecId(1)).unwrap();
        cx.load_lib(&lib).unwrap();
        let request = request_with(
            ReadEvalSource::Text("\"ok\"".to_owned()),
            Arc::new(ExprKindShape::new(ExprKind::String)),
        );

        let value = ReadEvalBroker.admit(&mut cx, request).unwrap();

        assert_eq!(
            value.object().as_expr(&mut cx).unwrap(),
            Expr::String("ok".to_owned())
        );
    }

    #[test]
    fn bytes_source_decodes_through_named_codec() {
        let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
        seat.grant(&mut cx, read_eval_capability());
        let lib = LispCodecLib::new(sim_kernel::CodecId(1)).unwrap();
        cx.load_lib(&lib).unwrap();
        let request = request_with(ReadEvalSource::Bytes(b"nil".to_vec()), Arc::new(AnyShape));

        let value = ReadEvalBroker.admit(&mut cx, request).unwrap();

        assert_eq!(value.object().as_expr(&mut cx).unwrap(), Expr::Nil);
    }
}
