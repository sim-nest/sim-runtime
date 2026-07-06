use std::sync::Arc;

use sim_codec::{Input, decode_tree_with_codec};
use sim_kernel::{
    Cx, Error, Expr, ReadPolicy, Ref, Symbol, TrustLevel, Value,
    capability::{control_capture_capability, control_resume_capability},
    control::{control_captured_status, control_result_status, control_resumed_status},
};
use sim_lib_control::{ConditionHandler, install_control_policy};
use sim_lib_namespace::NamespaceKind;
use sim_lib_standard_core::ProfileRegistry;
use sim_shape::{AnyShape, ExprKindShape};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn read_policy() -> ReadPolicy {
    ReadPolicy {
        trust: TrustLevel::TrustedSource,
        capabilities: sim_kernel::CapabilitySet::new(),
    }
}

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

fn string_body(label: &'static str) -> sim_lib_dispatch::MethodBody {
    Arc::new(move |cx, _args| cx.factory().string(label.to_owned()))
}

fn string_shape() -> Arc<dyn sim_kernel::Shape> {
    Arc::new(ExprKindShape::new(sim_kernel::ExprKind::String))
}

fn any_shape() -> Arc<dyn sim_kernel::Shape> {
    Arc::new(AnyShape)
}

#[test]
fn cl_reader_decodes_forms_with_locations() {
    let mut cx = cx();
    let codec_id = cx.registry_mut().fresh_codec_id();
    cx.load_lib(&ClLiteReaderCodecLib::new(codec_id)).unwrap();

    let tree = decode_tree_with_codec(
        &mut cx,
        &cl_reader_symbol(),
        Input::Text("(defun add1 (x) (+ x 1))".to_owned()),
        read_policy(),
        "unit.lisp",
    )
    .unwrap();

    assert_eq!(tree.origin.as_ref().unwrap().source.0.as_str(), "unit.lisp");
    let Expr::List(items) = &tree.expr else {
        panic!("expected CL-lite list");
    };
    assert_eq!(items[0], Expr::Symbol(Symbol::new("defun")));
    assert_eq!(items.len(), 4);
    assert_eq!(tree.children.len(), 4);

    let quoted = decode_tree_with_codec(
        &mut cx,
        &cl_reader_symbol(),
        Input::Text("'(:ok t nil)".to_owned()),
        read_policy(),
        "quote.lisp",
    )
    .unwrap();
    let Expr::List(items) = &quoted.expr else {
        panic!("expected quote list");
    };
    assert_eq!(items[0], Expr::Symbol(Symbol::new("quote")));
}

#[test]
fn cl_string_literal_preserves_non_ascii() {
    let mut cx = cx();
    let codec_id = cx.registry_mut().fresh_codec_id();
    cx.load_lib(&ClLiteReaderCodecLib::new(codec_id)).unwrap();

    // Source holds a 2-byte UTF-8 scalar inside the quotes; kept ASCII-only in
    // this file via a `\u{..}` escape (R8). It must decode, not turn to mojibake.
    let tree = decode_tree_with_codec(
        &mut cx,
        &cl_reader_symbol(),
        Input::Text("\"caf\u{00e9}\"".to_owned()),
        read_policy(),
        "unicode.lisp",
    )
    .unwrap();
    assert_eq!(tree.expr, Expr::String("caf\u{00e9}".to_owned()));
}

#[test]
fn cl_form_specs_name_the_shared_organs() {
    let specs = cl_lite_form_specs();
    assert!(specs.iter().any(|spec| {
        spec.symbol == Symbol::qualified("cl", "defun")
            && spec.role == ClLiteFormRole::Binding
            && spec.organ == sim_lib_binding::binding_organ_symbol()
    }));
    assert!(specs.iter().any(|spec| {
        spec.symbol == Symbol::qualified("cl", "handler-case")
            && spec.role == ClLiteFormRole::Control
            && spec.organ == sim_lib_control::control_organ_symbol()
    }));
    assert!(specs.iter().any(|spec| {
        spec.symbol == Symbol::qualified("cl", "defmethod")
            && spec.role == ClLiteFormRole::Dispatch
            && spec.organ == sim_lib_dispatch::dispatch_organ_symbol()
    }));
    assert!(specs.iter().any(|spec| {
        spec.symbol == Symbol::qualified("cl", "setf")
            && spec.role == ClLiteFormRole::Mutation
            && spec.organ == sim_lib_mutation::mutation_organ_symbol()
    }));
}

#[test]
fn defun_defmacro_let_and_setq_delegate_to_shared_organs() {
    let mut cx = cx();
    let mut runtime = ClLiteRuntime::new().unwrap();

    let identity = Symbol::qualified("cl-user", "identity");
    runtime
        .defun(
            &mut cx,
            identity.clone(),
            Arc::new(|_cx, _env, args| Ok(args[0].clone())),
        )
        .unwrap();
    let input = string(&mut cx, "kept");
    let output = call_cl_value(&mut cx, &runtime.function(&identity).unwrap(), vec![input])
        .unwrap()
        .object()
        .as_expr(&mut cx)
        .unwrap();
    assert_eq!(output, Expr::String("kept".to_owned()));

    let macro_name = Symbol::qualified("cl-user", "when");
    runtime
        .defmacro(
            &mut cx,
            macro_name.clone(),
            Arc::new(|cx, _env, _args| cx.factory().symbol(Symbol::new("expanded"))),
        )
        .unwrap();
    let expansion = call_cl_value(
        &mut cx,
        &runtime.macro_function(&macro_name).unwrap(),
        Vec::new(),
    )
    .unwrap()
    .object()
    .as_expr(&mut cx)
    .unwrap();
    assert_eq!(expansion, Expr::Symbol(Symbol::new("expanded")));

    let local_name = Symbol::new("x");
    let local = string(&mut cx, "lexical");
    let let_result = runtime
        .let_form(&mut cx, vec![(local_name.clone(), local)], |_cx, env| {
            env.lookup(&local_name)
        })
        .unwrap()
        .object()
        .as_expr(&mut cx)
        .unwrap();
    assert_eq!(let_result, Expr::String("lexical".to_owned()));

    let var = Symbol::new("*state*");
    let initial = string(&mut cx, "old");
    runtime
        .define_variable(&mut cx, var.clone(), initial)
        .unwrap();
    let denied_value = string(&mut cx, "denied");
    let denied = runtime.setq(&mut cx, &var, denied_value).unwrap_err();
    assert!(matches!(
        denied,
        Error::CapabilityDenied { capability } if capability == sim_lib_mutation::standard_mutate_capability()
    ));

    cx.grant(sim_lib_mutation::standard_mutate_capability());
    let updated = string(&mut cx, "new");
    runtime.setq(&mut cx, &var, updated).unwrap();
    let stored = runtime
        .variable_value(&var)
        .unwrap()
        .object()
        .as_expr(&mut cx)
        .unwrap();
    assert_eq!(stored, Expr::String("new".to_owned()));
}

#[test]
fn handler_case_and_restart_case_use_control_organ() {
    let mut cx = cx();
    install_control_policy(&mut cx);
    cx.grant(control_capture_capability());
    cx.grant(control_resume_capability());

    let mut scope = ClLiteControlScope::new();
    let kind = Symbol::qualified("condition", "file-error");
    let continuation = Ref::Symbol(Symbol::qualified("continuation", "handler"));
    scope.push_handler(ConditionHandler::new(
        kind.clone(),
        Ref::Symbol(Symbol::qualified("prompt", "handler")),
        continuation.clone(),
    ));

    let captured = scope
        .handler_case(
            &mut cx,
            kind,
            Ref::Symbol(Symbol::qualified("payload", "missing-file")),
        )
        .unwrap();
    assert_eq!(
        control_result_status(&cx, captured.capture_result()).unwrap(),
        Some(control_captured_status())
    );
    assert_eq!(captured.continuation(), &continuation);

    let restart = Symbol::qualified("restart", "use-value");
    scope.push_restart(restart.clone(), captured);
    let resumed = scope
        .restart_case(
            &mut cx,
            &restart,
            Ref::Symbol(Symbol::qualified("value", "fallback")),
        )
        .unwrap();
    assert_eq!(
        control_result_status(&cx, resumed.reference()).unwrap(),
        Some(control_resumed_status())
    );
}

#[test]
fn generic_functions_use_dispatch_organ() {
    let mut cx = cx();
    let mut generic = ClGenericFunction::new(Symbol::qualified("cl", "describe"));
    generic
        .add_primary_method(
            Symbol::qualified("method", "broad"),
            vec![any_shape()],
            string_body("broad"),
        )
        .unwrap();
    generic
        .add_primary_method(
            Symbol::qualified("method", "string"),
            vec![string_shape()],
            string_body("string"),
        )
        .unwrap();

    let args = [string(&mut cx, "sample")];
    let selected = generic.select_primary(&mut cx, &args).unwrap();
    assert_eq!(selected.method(), &Symbol::qualified("method", "string"));
    assert_eq!(
        generic.dispatch_order(&mut cx, &args).unwrap(),
        vec![Symbol::qualified("method", "string")]
    );
    assert_eq!(
        generic
            .call(&mut cx, &args)
            .unwrap()
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::String("string".to_owned())
    );
}

#[test]
fn profile_publishes_package_and_honest_clos_mop_badge() {
    let mut cx = cx();
    let mut registry = ProfileRegistry::new();
    let profile = install_cl_lite_profile(&mut cx, &mut registry).unwrap();

    assert!(registry.profile(&profile.symbol).is_some());
    assert!(
        profile
            .organs
            .iter()
            .any(|organ| organ.organ == sim_lib_control::control_organ_symbol())
    );
    assert!(
        profile
            .fidelity_badges
            .iter()
            .any(|badge| { badge.badge == cl_clos_mop_fidelity_symbol() && badge.level == 0 })
    );

    let clos_badges = cx
        .query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(profile.symbol.clone())),
            predicate: Some(Symbol::qualified("standard", "fidelity-badge")),
            object: Some(Ref::Symbol(cl_clos_mop_fidelity_symbol())),
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(clos_badges.len(), 1);

    let package = cl_lite_package().unwrap();
    assert_eq!(package.symbol(), &cl_lite_package_symbol());
    assert_eq!(package.kind(), NamespaceKind::Package);
    assert!(
        package
            .exported_entry(&Symbol::new("defun"))
            .unwrap()
            .target()
            == &Symbol::qualified("cl", "defun")
    );
}
