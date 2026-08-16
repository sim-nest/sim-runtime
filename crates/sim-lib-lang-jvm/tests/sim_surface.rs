use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, NumberLiteral, Symbol};
use sim_lib_lang_jvm::{
    JVM_DECLARED_ABSENCES, install_jvm_language_lib, jvm_browse_capability, jvm_invoke_capability,
    jvm_language_profile,
};

fn cx_with(capabilities: &[sim_kernel::CapabilityName]) -> Cx {
    let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    for capability in capabilities {
        seat.grant(&mut cx, capability.clone()).unwrap();
    }
    cx
}

fn call(name: &str, args: Vec<Expr>) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("jvm", name))),
        args,
    }
}

fn int(value: i32) -> Expr {
    Expr::Number(NumberLiteral {
        domain: Symbol::qualified("jvm", "int"),
        canonical: value.to_string(),
    })
}

#[test]
fn lisp_surface_defines_authorized_bytes_and_invokes_static_method() {
    let mut cx = cx_with(&[
        sim_lib_lang_jvm::class_load_capability(),
        jvm_invoke_capability(),
    ]);
    install_jvm_language_lib(&mut cx).unwrap();

    cx.eval_expr(call(
        "define",
        vec![
            Expr::String("StaticInt".into()),
            Expr::Bytes(include_bytes!("../fixtures/javac/StaticInt.class").to_vec()),
        ],
    ))
    .unwrap();
    let result = cx
        .eval_expr(call(
            "invoke-static",
            vec![
                Expr::String("StaticInt".into()),
                Expr::String("wholePipeline".into()),
                Expr::String("(II)I".into()),
                int(3),
                int(4),
            ],
        ))
        .unwrap();
    let Expr::Number(number) = result.object().as_expr(&mut cx).unwrap() else {
        panic!()
    };
    assert_eq!(number.canonical, "14");
}

#[test]
fn fidelity_names_three_absences_before_positive_evidence() {
    let profile = jvm_language_profile();
    let gaps = profile
        .unsupported_forms
        .iter()
        .map(Symbol::to_string)
        .collect::<Vec<_>>();
    assert_eq!(gaps, JVM_DECLARED_ABSENCES.map(|gap| format!("jvm/{gap}")));
    assert_eq!(profile.fidelity_badges.len(), 1);

    let mut cx = cx_with(&[]);
    install_jvm_language_lib(&mut cx).unwrap();
    let value = cx.eval_expr(call("fidelity", vec![])).unwrap();
    let Expr::List(items) = value.object().as_expr(&mut cx).unwrap() else {
        panic!()
    };
    assert_eq!(
        items[..3],
        JVM_DECLARED_ABSENCES
            .into_iter()
            .map(|gap| Expr::Symbol(Symbol::qualified("jvm", gap)))
            .collect::<Vec<_>>()
    );
}

#[test]
fn browsing_is_capability_gated_and_bounded() {
    let mut cx = cx_with(&[]);
    install_jvm_language_lib(&mut cx).unwrap();
    assert!(cx.eval_expr(call("browse", vec![int(1)])).is_err());
    let mut cx = cx_with(&[jvm_browse_capability()]);
    install_jvm_language_lib(&mut cx).unwrap();
    let value = cx.eval_expr(call("browse", vec![int(1)])).unwrap();
    let Expr::List(rows) = value.object().as_expr(&mut cx).unwrap() else {
        panic!()
    };
    assert!(rows.len() <= 1);
}
