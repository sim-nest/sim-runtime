use std::sync::Arc;

use sim_kernel::{
    Callable, ClaimPattern, Cx, DefaultFactory, Error, NoopEvalPolicy, Ref, Symbol,
    capability::{
        control_capture_capability, control_multishot_capability, control_prompt_capability,
        control_resume_capability,
    },
    card::{card_for_ref, card_kind_predicate},
    control::{
        ControlCapture, ControlPrompt, ControlResume, capture, control_aborted_status,
        control_captured_status, control_result_status, control_resumed_status,
        default_control_prompt, default_control_result_shape, prompt, resume,
    },
    standard::standard_organ_kind,
};

use super::*;

fn cx() -> Cx {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    install_control_policy(&mut cx);
    cx
}

fn symbol_ref(namespace: &str, name: &str) -> Ref {
    Ref::Symbol(Symbol::qualified(namespace, name))
}

fn symbol_value(cx: &mut Cx, namespace: &str, name: &str) -> sim_kernel::Value {
    cx.factory()
        .symbol(Symbol::qualified(namespace, name))
        .unwrap()
}

fn symbol_args(cx: &mut Cx, refs: &[(&str, &str)]) -> sim_kernel::Args {
    sim_kernel::Args::new(
        refs.iter()
            .map(|(namespace, name)| symbol_value(cx, namespace, name))
            .collect(),
    )
}

struct LibraryPrompt;

impl crate::ControlPrompt for LibraryPrompt {
    fn tag(&self) -> crate::ControlTag {
        crate::ControlTag::new(Symbol::qualified("test", "library-prompt"))
    }

    fn input(&self) -> Ref {
        symbol_ref("test", "library-input")
    }
}

#[test]
fn one_shot_prompt_body_can_return_normally() {
    let mut cx = cx();
    cx.grant(control_prompt_capability());
    let expected = symbol_ref("test", "prompt-result");

    let actual = prompt(
        &mut cx,
        ControlPrompt::new(
            default_control_prompt(),
            symbol_ref("test", "input"),
            default_control_result_shape(),
        ),
        {
            let expected = expected.clone();
            move |_cx| Ok(expected)
        },
    )
    .unwrap();

    assert_eq!(actual, expected);
}

#[test]
fn library_prompt_trait_raises_kernel_prompt() {
    let mut cx = cx();
    cx.grant(control_prompt_capability());

    let result = raise_prompt(&mut cx, &LibraryPrompt).unwrap();

    assert_eq!(result, symbol_ref("test", "library-input"));
}

#[test]
fn one_shot_capture_returns_captured_result() {
    let mut cx = cx();
    cx.grant(control_capture_capability());

    let result = capture(
        &mut cx,
        ControlCapture::new(
            default_control_prompt(),
            symbol_ref("test", "continuation"),
            symbol_ref("test", "value"),
            default_control_result_shape(),
        ),
    )
    .unwrap();

    assert_eq!(
        control_result_status(&cx, &result).unwrap(),
        Some(control_captured_status())
    );
}

#[test]
fn one_shot_resume_consumes_continuation() {
    let mut cx = cx();
    cx.grant(control_resume_capability());
    let continuation = symbol_ref("test", "continuation");

    let result = resume(
        &mut cx,
        ControlResume::new(
            continuation.clone(),
            symbol_ref("test", "first"),
            default_control_result_shape(),
        ),
    )
    .unwrap();

    assert_eq!(
        control_result_status(&cx, &result).unwrap(),
        Some(control_resumed_status())
    );

    let err = resume(
        &mut cx,
        ControlResume::new(
            continuation,
            symbol_ref("test", "second"),
            default_control_result_shape(),
        ),
    )
    .unwrap_err();

    assert!(matches!(err, Error::Eval(message) if message.contains("already resumed")));
}

#[test]
fn control_lib_registers_public_ops_and_claims() {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));

    install_control_lib(&mut cx).unwrap();
    install_control_lib(&mut cx).unwrap();

    for symbol in [
        prompt_symbol(),
        capture_symbol(),
        abort_symbol(),
        resume_symbol(),
    ] {
        assert!(cx.resolve_function(&symbol).is_ok());
    }

    let card = card_for_ref(&mut cx, Ref::Symbol(control_organ_symbol()))
        .unwrap()
        .object()
        .as_expr(&mut cx)
        .unwrap();
    assert_eq!(
        table_value(&card, "kind"),
        Some(&sim_kernel::Expr::Symbol(standard_organ_kind()))
    );
    assert_list_contains_symbol(
        table_value(&card, "ops").unwrap(),
        Symbol::qualified("control", "prompt.v1"),
    );
}

#[test]
fn control_lib_claims_unload_and_reload() {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));

    install_control_lib(&mut cx).unwrap();
    let lib_id = cx.registry().lib(&manifest_name()).unwrap().id;
    assert!(cx.resolve_function(&prompt_symbol()).is_ok());
    assert!(!control_organ_kind_claims(&cx).is_empty());

    cx.unload_lib(lib_id).unwrap();
    assert!(cx.resolve_function(&prompt_symbol()).is_err());
    assert!(control_organ_kind_claims(&cx).is_empty());

    install_control_lib(&mut cx).unwrap();
    assert!(cx.resolve_function(&prompt_symbol()).is_ok());
    assert!(!control_organ_kind_claims(&cx).is_empty());
}

#[test]
fn public_prompt_op_returns_normally() {
    let mut cx = cx();
    cx.grant(control_prompt_capability());
    let prompt_fn = ControlFunction::prompt();
    let args = symbol_args(&mut cx, &[("test", "prompt"), ("test", "value")]);

    let result = prompt_fn.call(&mut cx, args).unwrap();
    let result = result
        .object()
        .downcast_ref::<ControlResultValue>()
        .unwrap();

    assert_eq!(result.reference(), &symbol_ref("test", "value"));
}

#[test]
fn public_abort_op_returns_aborted_result() {
    let mut cx = cx();
    cx.grant(control_capture_capability());
    let abort_fn = ControlFunction::abort();
    let args = symbol_args(&mut cx, &[("test", "prompt"), ("test", "abort-value")]);

    let result = abort_fn.call(&mut cx, args).unwrap();
    let result = result
        .object()
        .downcast_ref::<ControlResultValue>()
        .unwrap();

    assert_eq!(
        control_result_status(&cx, result.reference()).unwrap(),
        Some(control_aborted_status())
    );
}

#[test]
fn public_capture_op_returns_continuation_value_and_resume_consumes_it() {
    let mut cx = cx();
    cx.grant(control_capture_capability());
    cx.grant(control_resume_capability());
    let capture_fn = ControlFunction::capture();
    let resume_fn = ControlFunction::resume();
    let capture_args = symbol_args(
        &mut cx,
        &[
            ("test", "prompt"),
            ("test", "continuation"),
            ("test", "captured"),
        ],
    );

    let continuation = capture_fn.call(&mut cx, capture_args).unwrap();
    let continuation_data = continuation
        .object()
        .downcast_ref::<ContinuationValue>()
        .unwrap();
    assert_eq!(
        continuation_data.continuation(),
        &symbol_ref("test", "continuation")
    );
    assert_eq!(
        control_result_status(&cx, continuation_data.capture_result()).unwrap(),
        Some(control_captured_status())
    );

    let resume_value = symbol_value(&mut cx, "test", "resumed");
    let resumed = resume_fn
        .call(
            &mut cx,
            sim_kernel::Args::new(vec![continuation.clone(), resume_value]),
        )
        .unwrap();
    let resumed = resumed
        .object()
        .downcast_ref::<ControlResultValue>()
        .unwrap();
    assert_eq!(
        control_result_status(&cx, resumed.reference()).unwrap(),
        Some(control_resumed_status())
    );

    let resume_again = symbol_value(&mut cx, "test", "resumed-again");
    let err = resume_fn
        .call(
            &mut cx,
            sim_kernel::Args::new(vec![continuation, resume_again]),
        )
        .unwrap_err();
    assert!(matches!(err, Error::Eval(message) if message.contains("already resumed")));
}

#[test]
fn multishot_capture_requires_multishot_capability() {
    let mut cx = cx();
    cx.grant(control_capture_capability());
    let capture_fn = ControlFunction::capture();
    let mut values = symbol_args(
        &mut cx,
        &[
            ("test", "prompt"),
            ("test", "continuation"),
            ("test", "captured"),
        ],
    )
    .into_vec();
    values.push(cx.factory().bool(true).unwrap());
    let args = sim_kernel::Args::new(values);

    let err = capture_fn.call(&mut cx, args).unwrap_err();

    assert!(matches!(
        err,
        Error::CapabilityDenied { capability } if capability == control_multishot_capability()
    ));
    assert_eq!(cx.effect_ledger().records().len(), 1);
    assert!(cx.effect_ledger().records()[0].aborted);
}

#[test]
fn effects_are_recorded_before_policy_execution() {
    struct LedgerCheckingPolicy;

    impl sim_kernel::control::ControlPolicy for LedgerCheckingPolicy {
        fn name(&self) -> &'static str {
            "ledger-checking-control"
        }

        fn capture(
            &self,
            cx: &mut Cx,
            _capture: &sim_kernel::control::ControlCapture,
        ) -> sim_kernel::Result<Ref> {
            if cx.effect_ledger().records().len() == 1 {
                return Err(Error::Eval("policy saw requested effect".to_owned()));
            }
            Err(Error::Eval("effect was not recorded first".to_owned()))
        }
    }

    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    cx.set_control_policy(Arc::new(LedgerCheckingPolicy));
    cx.grant(control_capture_capability());
    let capture_fn = ControlFunction::capture();
    let args = symbol_args(
        &mut cx,
        &[
            ("test", "prompt"),
            ("test", "continuation"),
            ("test", "captured"),
        ],
    );

    let err = capture_fn.call(&mut cx, args).unwrap_err();

    assert!(matches!(err, Error::Eval(message) if message == "policy saw requested effect"));
    assert_eq!(cx.effect_ledger().records().len(), 1);
    assert!(cx.effect_ledger().records()[0].aborted);
}

#[test]
fn segmented_policy_prototype_delegates_one_shot_behavior() {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    cx.set_control_policy(segmented_control_policy(symbol_ref("segment", "root")));
    cx.grant(control_prompt_capability());

    let result = prompt(
        &mut cx,
        ControlPrompt::new(
            default_control_prompt(),
            symbol_ref("test", "input"),
            default_control_result_shape(),
        ),
        |_cx| Ok(symbol_ref("test", "segmented-result")),
    )
    .unwrap();

    assert_eq!(result, symbol_ref("test", "segmented-result"));
}

fn table_value<'a>(expr: &'a sim_kernel::Expr, key: &str) -> Option<&'a sim_kernel::Expr> {
    let sim_kernel::Expr::Map(entries) = expr else {
        return None;
    };
    entries.iter().find_map(|(entry_key, entry_value)| {
        let sim_kernel::Expr::Symbol(entry_key) = entry_key else {
            return None;
        };
        (entry_key == &Symbol::new(key)).then_some(entry_value)
    })
}

fn assert_list_contains_symbol(expr: &sim_kernel::Expr, expected: Symbol) {
    let sim_kernel::Expr::List(items) = expr else {
        panic!("expected list");
    };
    assert!(
        items
            .iter()
            .any(|item| item == &sim_kernel::Expr::Symbol(expected.clone())),
        "expected list to contain {expected}"
    );
}

fn control_organ_kind_claims(cx: &Cx) -> Vec<sim_kernel::Claim> {
    cx.query_facts(ClaimPattern::exact(
        Ref::Symbol(control_organ_symbol()),
        card_kind_predicate(),
        Ref::Symbol(standard_organ_kind()),
    ))
    .unwrap()
}

// ---- COOKBOOK_7 COOK7.02: the `if` eval-policy organ (special form) ----

#[test]
fn if_special_form_selects_branch_and_is_lazy() {
    use sim_kernel::{EagerPolicy, Expr};

    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    crate::install_control_lib(&mut cx).unwrap();

    let if_call = |args: Vec<Expr>| Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::new("if"))),
        args,
    };
    let s = |text: &str| Expr::String(text.to_owned());

    // Truthy test -> then-branch.
    let taken = cx
        .eval_expr(if_call(vec![Expr::Bool(true), s("then"), s("else")]))
        .unwrap();
    assert_eq!(taken.object().as_expr(&mut cx).unwrap(), s("then"));

    // Falsy test -> else-branch.
    let alt = cx
        .eval_expr(if_call(vec![Expr::Bool(false), s("then"), s("else")]))
        .unwrap();
    assert_eq!(alt.object().as_expr(&mut cx).unwrap(), s("else"));

    // Missing else on a falsy test -> nil.
    let none = cx
        .eval_expr(if_call(vec![Expr::Bool(false), s("then")]))
        .unwrap();
    assert_eq!(none.object().as_expr(&mut cx).unwrap(), Expr::Nil);

    // Laziness: the untaken branch is never evaluated. Here the else-branch is a
    // nested `(if)` with a bad arity that would error if evaluated; since the
    // test is truthy it is not, so the whole form still yields the then-branch.
    let lazy = cx
        .eval_expr(if_call(vec![Expr::Bool(true), s("ok"), if_call(vec![])]))
        .unwrap();
    assert_eq!(lazy.object().as_expr(&mut cx).unwrap(), s("ok"));

    // A bad arity on the outer form is a real error.
    let err = cx.eval_expr(if_call(vec![Expr::Bool(true)])).unwrap_err();
    assert!(matches!(err, Error::Eval(msg) if msg.contains("if expects")));
}
