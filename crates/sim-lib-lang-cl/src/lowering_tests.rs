use std::sync::Arc;

use sim_codec::{Input, decode_tree_with_codec};
use sim_kernel::{Cx, Expr, ReadPolicy, Symbol, TrustLevel};
use sim_lib_binding::{BindingProfileModes, HygieneMode};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn read_policy() -> ReadPolicy {
    ReadPolicy {
        trust: TrustLevel::TrustedSource,
        capabilities: sim_kernel::CapabilitySet::new(),
    }
}

fn decode_tree(cx: &mut Cx, source: &str, source_id: &str) -> sim_kernel::LocatedExprTree {
    if cx.resolve_codec(&cl_reader_symbol()).is_err() {
        let codec_id = cx.registry_mut().fresh_codec_id();
        cx.load_lib(&ClLiteReaderCodecLib::new(codec_id)).unwrap();
    }
    decode_tree_with_codec(
        cx,
        &cl_reader_symbol(),
        Input::Text(source.to_owned()),
        read_policy(),
        source_id,
    )
    .unwrap()
}

fn let_binding_symbol(expr: &Expr) -> Symbol {
    let Expr::List(items) = expr else {
        panic!("expected list expansion");
    };
    let Expr::List(bindings) = &items[1] else {
        panic!("expected let bindings");
    };
    let Expr::List(pair) = &bindings[0] else {
        panic!("expected binding pair");
    };
    let Expr::Symbol(symbol) = &pair[0] else {
        panic!("expected binding symbol");
    };
    symbol.clone()
}

fn let_body_symbol(expr: &Expr) -> Symbol {
    let Expr::List(items) = expr else {
        panic!("expected list expansion");
    };
    let Expr::Symbol(symbol) = &items[2] else {
        panic!("expected body symbol");
    };
    symbol.clone()
}

#[test]
fn source_macro_expansion_uses_macro_table() {
    let mut cx = cx();
    let mut runtime = ClLiteRuntime::new().unwrap();
    runtime
        .defmacro(
            &mut cx,
            Symbol::new("wrap-source"),
            Arc::new(|cx, _env, args| {
                let arg = args[0].object().as_expr(cx).unwrap();
                cx.factory()
                    .expr(Expr::List(vec![Expr::Symbol(Symbol::new("wrapped")), arg]))
            }),
        )
        .unwrap();

    let tree = decode_tree(&mut cx, "(wrap-source x)", "macro.lisp");
    let expanded =
        expand_cl_lite_tree(&mut cx, &runtime, &tree, BindingProfileModes::default()).unwrap();

    assert_eq!(
        expanded.expr,
        Expr::List(vec![
            Expr::Symbol(Symbol::new("wrapped")),
            Expr::Symbol(Symbol::new("x")),
        ])
    );
    assert_eq!(expanded.origin, tree.origin);
}

#[test]
fn hygiene_modes_control_name_capture_during_source_expansion() {
    let mut cx = cx();
    let mut runtime = ClLiteRuntime::new().unwrap();
    runtime
        .defmacro(
            &mut cx,
            Symbol::new("with-temp"),
            Arc::new(|cx, _env, args| {
                let value = args[0].object().as_expr(cx).unwrap();
                let body = args[1].object().as_expr(cx).unwrap();
                cx.factory().expr(Expr::List(vec![
                    Expr::Symbol(Symbol::new("let")),
                    Expr::List(vec![Expr::List(vec![
                        Expr::Symbol(Symbol::new("temp")),
                        value,
                    ])]),
                    body,
                ]))
            }),
        )
        .unwrap();
    runtime
        .defmacro(
            &mut cx,
            Symbol::new("with-explicit-temp"),
            Arc::new(|cx, _env, args| {
                let value = args[0].object().as_expr(cx).unwrap();
                let temp = Expr::Symbol(cl_explicit_hygiene_symbol("temp"));
                cx.factory().expr(Expr::List(vec![
                    Expr::Symbol(Symbol::new("let")),
                    Expr::List(vec![Expr::List(vec![temp.clone(), value])]),
                    temp,
                ]))
            }),
        )
        .unwrap();

    let capture_tree = decode_tree(&mut cx, "(with-temp x temp)", "capture.lisp");
    let hygienic = expand_cl_lite_tree(
        &mut cx,
        &runtime,
        &capture_tree,
        BindingProfileModes::default(),
    )
    .unwrap();
    let unhygienic = expand_cl_lite_tree(
        &mut cx,
        &runtime,
        &capture_tree,
        BindingProfileModes {
            hygiene: HygieneMode::Unhygienic,
            ..BindingProfileModes::default()
        },
    )
    .unwrap();

    assert_ne!(let_binding_symbol(&hygienic.expr), Symbol::new("temp"));
    assert_eq!(let_body_symbol(&hygienic.expr), Symbol::new("temp"));
    assert_eq!(let_binding_symbol(&unhygienic.expr), Symbol::new("temp"));
    assert_eq!(let_body_symbol(&unhygienic.expr), Symbol::new("temp"));

    let explicit_tree = decode_tree(&mut cx, "(with-explicit-temp x)", "explicit.lisp");
    let explicit = expand_cl_lite_tree(
        &mut cx,
        &runtime,
        &explicit_tree,
        BindingProfileModes {
            hygiene: HygieneMode::Explicit,
            ..BindingProfileModes::default()
        },
    )
    .unwrap();

    let binding = let_binding_symbol(&explicit.expr);
    let body = let_body_symbol(&explicit.expr);
    assert_ne!(binding, Symbol::new("temp"));
    assert_eq!(binding, body);
}
