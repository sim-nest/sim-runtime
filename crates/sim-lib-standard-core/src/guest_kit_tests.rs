use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy, Symbol, Value};

use crate::{
    Arity, CoercionPolicy, GuestRuntimeKit, LanguageProfile, OrganUse, SharedOrganRuntime,
    TruthPolicy, sim_expression_profile,
};

#[test]
fn guest_runtime_kits_cover_distinct_truth_and_arity_rules() {
    let mut cx = test_cx();
    let first_profile = sample_profile();
    let second_profile = sim_expression_profile();
    let mut runtime = SharedOrganRuntime::new();
    runtime.register_profile(first_profile.clone()).unwrap();
    runtime.register_profile(second_profile.clone()).unwrap();

    let nil = cx.factory().nil().unwrap();
    let false_value = cx.factory().bool(false).unwrap();
    let true_value = cx.factory().bool(true).unwrap();
    let text_value = cx.factory().string("value".to_owned()).unwrap();
    let nil_and_false_kit = GuestRuntimeKit::new(
        Arc::new(NilAndFalseFalsey),
        Arc::new(NoBoundaryCoercion),
        nil.clone(),
    );
    let false_only_kit = GuestRuntimeKit::new(
        Arc::new(FalseOnlyFalsey),
        Arc::new(NoBoundaryCoercion),
        nil.clone(),
    );

    runtime
        .register_kit(&first_profile.symbol, nil_and_false_kit)
        .unwrap();
    runtime
        .register_kit(&second_profile.symbol, false_only_kit)
        .unwrap();

    let first_kit = runtime.kit(&first_profile.symbol).unwrap().clone();
    assert!(!first_kit.is_truthy(&mut cx, &nil).unwrap());
    assert!(!first_kit.is_truthy(&mut cx, &false_value).unwrap());
    assert!(first_kit.is_truthy(&mut cx, &text_value).unwrap());
    assert_eq!(
        first_kit.adjust_values(
            vec![text_value.clone(), true_value.clone()],
            Arity::Exact(1)
        ),
        vec![text_value.clone()]
    );

    let second_kit = runtime.kit(&second_profile.symbol).unwrap().clone();
    assert!(second_kit.is_truthy(&mut cx, &nil).unwrap());
    assert!(!second_kit.is_truthy(&mut cx, &false_value).unwrap());
    assert!(second_kit.is_truthy(&mut cx, &true_value).unwrap());
    assert_eq!(
        second_kit.adjust_values(Vec::new(), Arity::AtLeastOne),
        vec![nil]
    );
    assert_eq!(
        second_kit.adjust_values(vec![text_value.clone(), true_value.clone()], Arity::All),
        vec![text_value.clone(), true_value]
    );
    assert!(
        second_kit
            .to_number(&mut cx, &text_value)
            .unwrap()
            .is_none()
    );
    assert!(
        second_kit
            .to_string(&mut cx, &text_value)
            .unwrap()
            .is_none()
    );
}

fn sample_profile() -> LanguageProfile {
    LanguageProfile::new(Symbol::qualified("lang", "guest-kit-sample/v1"))
        .with_organ(OrganUse::new(Symbol::qualified("organ", "control")))
}

fn test_cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}

struct NilAndFalseFalsey;

impl TruthPolicy for NilAndFalseFalsey {
    fn is_truthy(&self, cx: &mut Cx, value: &Value) -> sim_kernel::Result<bool> {
        Ok(!matches!(
            value.object().as_expr(cx)?,
            Expr::Nil | Expr::Bool(false)
        ))
    }
}

struct FalseOnlyFalsey;

impl TruthPolicy for FalseOnlyFalsey {
    fn is_truthy(&self, cx: &mut Cx, value: &Value) -> sim_kernel::Result<bool> {
        Ok(!matches!(value.object().as_expr(cx)?, Expr::Bool(false)))
    }
}

struct NoBoundaryCoercion;

impl CoercionPolicy for NoBoundaryCoercion {
    fn to_number(&self, _cx: &mut Cx, _value: &Value) -> sim_kernel::Result<Option<Value>> {
        Ok(None)
    }

    fn to_string(&self, _cx: &mut Cx, _value: &Value) -> sim_kernel::Result<Option<Value>> {
        Ok(None)
    }
}
