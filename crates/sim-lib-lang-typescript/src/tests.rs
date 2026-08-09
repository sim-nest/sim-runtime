use std::sync::Arc;

use sim_kernel::{
    Args, Callable, Cx, DefaultFactory, Expr, HybridPolicy, Object, ObjectCompat, Result, Symbol,
};
use sim_lib_lang_javascript::{JavascriptEvalPolicy, JavascriptState, JavascriptValue};

use crate::{
    AnnotationMetadata, AnnotationProvenance, ProjectedShape, TypeScriptNotation,
    TypeScriptProgram, attach_browse_signature, project_annotation, typescript_gap_manifest,
    typescript_notation_profile,
};

fn token(text: &str) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("javascript", "token"))),
        args: vec![
            Expr::Symbol(Symbol::new("token")),
            Expr::String(text.into()),
            Expr::Bool(true),
        ],
    }
}
fn erased(tokens: &[&str]) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("javascript", "module"))),
        args: tokens.iter().map(|text| token(text)).collect(),
    }
}

#[test]
fn profile_is_one_notation_layer_over_the_exact_javascript_policy() {
    let typescript = typescript_notation_profile();
    let javascript = sim_lib_lang_javascript::javascript_core_profile();
    assert_eq!(
        typescript.symbol,
        Symbol::qualified("language", "typescript-notation")
    );
    assert_eq!(typescript.reader, Symbol::qualified("codec", "typescript"));
    assert_eq!(typescript.eval_policy, javascript.eval_policy);
    assert_eq!(typescript.organs, javascript.organs);
    assert_eq!(typescript.capabilities, javascript.capabilities);
    assert_eq!(
        typescript.unsupported_forms.len(),
        typescript_gap_manifest().len()
    );
}

#[test]
fn faithful_categories_project_without_checker_state() {
    assert!(matches!(
        project_annotation("boolean"),
        Some(ProjectedShape::Primitive(_))
    ));
    assert!(
        matches!(project_annotation("true | string"), Some(ProjectedShape::Union(x)) if x.len() == 2)
    );
    assert!(
        matches!(project_annotation("[number, string]"), Some(ProjectedShape::Tuple(x)) if x.len() == 2)
    );
    assert!(matches!(
        project_annotation("number[]"),
        Some(ProjectedShape::Array(_))
    ));
    for unsupported in [
        "T",
        "keyof T",
        "T extends U ? X : Y",
        "{ readonly x?: number }",
    ] {
        assert_eq!(project_annotation(unsupported), None);
    }
}

#[test]
fn erased_execution_has_identical_result_and_effects() {
    let graph = erased(&["let", "answer", "=", "40", "+", "2", ";", "answer", ";"]);
    let program = TypeScriptProgram {
        javascript: graph.clone(),
        annotations: vec![AnnotationMetadata {
            provenance: AnnotationProvenance {
                source: "number".into(),
                span: 12..18,
                context: vec!["variable".into()],
                origins: vec!["typescript".into(), "javascript".into()],
            },
            projected: project_annotation("number"),
        }],
    };
    let mut javascript_state = JavascriptState::default();
    let mut typescript_state = JavascriptState::default();
    let javascript_result = JavascriptEvalPolicy::new(64)
        .unwrap()
        .eval_lowered(&graph, &mut javascript_state)
        .unwrap();
    let typescript_result = TypeScriptNotation::new(64)
        .unwrap()
        .eval(&program, &mut typescript_state)
        .unwrap();
    assert_eq!(typescript_result, javascript_result);
    assert_eq!(
        typescript_state.get("answer"),
        javascript_state.get("answer")
    );
    assert_eq!(
        typescript_state.get("answer"),
        Some(&JavascriptValue::Number(42.0))
    );
}

struct ConstantCallable;
impl Object for ConstantCallable {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("constant".into())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
impl ObjectCompat for ConstantCallable {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}
impl Callable for ConstantCallable {
    fn call(&self, cx: &mut Cx, _args: Args) -> Result<sim_kernel::Value> {
        cx.factory().string("unchanged".into())
    }
}

#[test]
fn browse_signature_attaches_projection_without_dynamic_guard() {
    let mut cx = Cx::new(Arc::new(HybridPolicy), Arc::new(DefaultFactory));
    let callable = cx.factory().opaque(Arc::new(ConstantCallable)).unwrap();
    let wrapped = attach_browse_signature(
        &mut cx,
        callable,
        project_annotation("[number]"),
        project_annotation("string"),
    )
    .unwrap();
    let callable = wrapped.object().as_callable().unwrap();
    assert_eq!(
        callable
            .call(&mut cx, Args::new(Vec::new()))
            .unwrap()
            .object()
            .display(&mut cx)
            .unwrap(),
        "unchanged"
    );
    assert!(callable.browse_args_shape(&mut cx).unwrap().is_some());
    assert!(callable.browse_result_shape(&mut cx).unwrap().is_some());
}

#[test]
fn crate_has_no_independent_runtime_or_compiler_products() {
    let manifest = include_str!("../Cargo.toml");
    let source = concat!(
        include_str!("lib.rs"),
        include_str!("profile.rs"),
        include_str!("metadata.rs"),
        include_str!("runtime.rs")
    );
    for forbidden_dependency in ["typescript =", "swc", "deno", "node"] {
        assert!(!manifest.contains(forbidden_dependency));
    }
    for forbidden_declaration in [
        "struct TypeChecker",
        "struct ModuleCache",
        "enum TypeScriptValue",
        "struct CompilerDiagnostic",
        "struct TypeScriptEvalPolicy",
    ] {
        assert!(!source.contains(forbidden_declaration));
    }
    assert!(source.contains("JavascriptEvalPolicy"));
}
