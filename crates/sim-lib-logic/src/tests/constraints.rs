use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, DefaultFactory, EagerPolicy, Expr, Object, Ref, Result, Symbol,
    Value, capability::control_prompt_capability, effect::effect_control_prompt_kind,
    logic_tool_call_capability,
};

use crate::{LogicConfig, LogicDb, query::query_all};

fn number(text: &str) -> Expr {
    Expr::Number(sim_kernel::NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: text.to_owned(),
    })
}

#[test]
fn between_generates_bounded_answers() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("between")),
            number("1"),
            number("3"),
            Expr::Local(Symbol::new("x")),
        ]),
        Some(10),
    )
    .unwrap();
    assert_eq!(answers.len(), 3);
}

#[test]
fn plus_solves_one_unknown() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("plus")),
            number("2"),
            number("3"),
            Expr::Local(Symbol::new("x")),
        ]),
        Some(10),
    )
    .unwrap();
    assert_eq!(answers.len(), 1);
}

#[test]
fn clp_constraint_entails_and_records_control_prompt() {
    let mut cx = control_cx();
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("#=")),
            number("2"),
            number("2"),
        ]),
        Some(10),
    )
    .unwrap();

    assert_eq!(answers.len(), 1);
    assert_control_constraint_prompt(&cx, false);
}

#[test]
fn clp_constraint_disentails_and_records_control_prompt() {
    let mut cx = control_cx();
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("#<")),
            number("3"),
            number("2"),
        ]),
        Some(10),
    )
    .unwrap();

    assert!(answers.is_empty());
    assert_control_constraint_prompt(&cx, false);
}

#[test]
fn clp_constraint_residual_is_recorded_as_suspended_demand() {
    let mut cx = control_cx();
    let result = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("dif")),
            Expr::Local(Symbol::new("x")),
            number("1"),
        ]),
        Some(10),
    );

    assert!(
        matches!(result, Err(sim_kernel::Error::Eval(message)) if message.contains("residual constraint demand suspended"))
    );
    assert_control_constraint_prompt(&cx, false);
}

#[test]
fn clp_constraint_requires_control_prompt_capability() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_lib_control::install_control_policy(&mut cx);
    let denied = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("#=")),
            number("2"),
            number("2"),
        ]),
        Some(10),
    );

    assert!(matches!(
        denied,
        Err(sim_kernel::Error::CapabilityDenied { capability })
            if capability == control_prompt_capability()
    ));
    assert_control_constraint_prompt(&cx, true);
}

#[test]
fn tool_call_requires_capability_and_unifies_result() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let tool = cx.factory().opaque(Arc::new(EchoTool)).unwrap();
    cx.env_mut().define(Symbol::new("echo-tool"), tool);
    let denied = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("tool-call")),
            Expr::Symbol(Symbol::new("echo-tool")),
            Expr::List(vec![Expr::String("hello".to_owned())]),
            Expr::Local(Symbol::new("x")),
        ]),
        Some(10),
    );
    assert!(matches!(
        denied,
        Err(sim_kernel::Error::CapabilityDenied { capability })
            if capability == logic_tool_call_capability()
    ));

    cx.grant(logic_tool_call_capability());
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("tool-call")),
            Expr::Symbol(Symbol::new("echo-tool")),
            Expr::List(vec![Expr::String("hello".to_owned())]),
            Expr::Local(Symbol::new("x")),
        ]),
        Some(10),
    )
    .unwrap();
    assert_eq!(answers.len(), 1);
}

fn control_cx() -> Cx {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_lib_control::install_control_policy(&mut cx);
    cx.grant(control_prompt_capability());
    cx
}

fn assert_control_constraint_prompt(cx: &Cx, aborted: bool) {
    let records = cx.effect_ledger().records();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.aborted, aborted);
    let effect = cx
        .effect_ledger()
        .effect(&record.effect)
        .expect("effect request is stored");
    assert_eq!(effect.kind, effect_control_prompt_kind());
    assert_eq!(
        effect.subject,
        Ref::Symbol(Symbol::qualified("logic", "constraint"))
    );
    assert!(matches!(effect.input, Ref::Content(_)));
}

struct EchoTool;

impl Object for EchoTool {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<echo-tool>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for EchoTool {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for EchoTool {
    fn call(&self, _cx: &mut Cx, args: Args) -> Result<Value> {
        args.values()
            .first()
            .cloned()
            .ok_or_else(|| sim_kernel::Error::Eval("echo-tool expects one argument".to_owned()))
    }
}
