use std::sync::Arc;

use sim_kernel::{
    AbiVersion, CapabilityName, Cx, DefaultFactory, EagerPolicy, Export, Expr, Lib, LibManifest,
    LibTarget, Linker, LoadCx, MatchScore, Result, ShapeDoc, ShapeMatch, Symbol, Version,
};
use sim_shape::{Shape, shape_value};

use crate::{LogicConfig, LogicEnv, model::OccursCheck, unify::unify_exprs};

#[test]
fn unify_binds_repeated_variables_across_lists() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let left = Expr::List(vec![
        Expr::Symbol(Symbol::new("pair")),
        Expr::Local(Symbol::new("x")),
        Expr::Local(Symbol::new("x")),
    ]);
    let right = Expr::List(vec![
        Expr::Symbol(Symbol::new("pair")),
        Expr::Number(sim_kernel::NumberLiteral {
            domain: Symbol::qualified("numbers", "i64"),
            canonical: "1".to_owned(),
        }),
        Expr::Number(sim_kernel::NumberLiteral {
            domain: Symbol::qualified("numbers", "i64"),
            canonical: "1".to_owned(),
        }),
    ]);
    let matched = unify_exprs(&mut cx, &LogicConfig::default(), &left, &right).unwrap();
    assert!(matched.accepted);
    assert_eq!(matched.captures.exprs().len(), 1);
}

#[test]
fn occurs_check_rejects_cycles() {
    let mut env = LogicEnv::new();
    let value = Expr::List(vec![
        Expr::Symbol(Symbol::new("loop")),
        Expr::Local(Symbol::new("x")),
    ]);
    let err = env
        .bind(Symbol::new("x"), value, OccursCheck::Always)
        .unwrap_err();
    assert!(format!("{err}").contains("occurs check"));
}

#[test]
fn shape_unify_binds_logic_variable() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let mut env = LogicEnv::new();
    let pattern = Expr::List(vec![
        Expr::Symbol(Symbol::new("parent")),
        Expr::Symbol(Symbol::new("tom")),
        Expr::Local(Symbol::new("X")),
    ]);
    let subject = Expr::List(vec![
        Expr::Symbol(Symbol::new("parent")),
        Expr::Symbol(Symbol::new("tom")),
        Expr::Symbol(Symbol::new("bob")),
    ]);
    assert!(
        env.unify(&mut cx, &pattern, &subject, OccursCheck::Always)
            .unwrap()
    );
    assert_eq!(
        env.get(&Symbol::new("X")),
        Some(&Expr::Symbol(Symbol::new("bob")))
    );
}

#[test]
fn shape_unify_fails_on_mismatch() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let mut env = LogicEnv::new();
    let pattern = Expr::List(vec![
        Expr::Symbol(Symbol::new("parent")),
        Expr::Symbol(Symbol::new("tom")),
        Expr::Local(Symbol::new("X")),
    ]);
    let subject = Expr::List(vec![
        Expr::Symbol(Symbol::new("child")),
        Expr::Symbol(Symbol::new("tom")),
        Expr::Symbol(Symbol::new("bob")),
    ]);
    assert!(
        !env.unify(&mut cx, &pattern, &subject, OccursCheck::Always)
            .unwrap()
    );
    assert_eq!(env.get(&Symbol::new("X")), None);
}

#[test]
fn shape_unify_repeated_variable_requires_same_subject() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let mut accepted = LogicEnv::new();
    let pattern = Expr::List(vec![
        Expr::Symbol(Symbol::new("same")),
        Expr::Local(Symbol::new("X")),
        Expr::Local(Symbol::new("X")),
    ]);
    let same_subject = Expr::List(vec![
        Expr::Symbol(Symbol::new("same")),
        Expr::Symbol(Symbol::new("bob")),
        Expr::Symbol(Symbol::new("bob")),
    ]);
    assert!(
        accepted
            .unify(&mut cx, &pattern, &same_subject, OccursCheck::Always)
            .unwrap()
    );
    assert_eq!(
        accepted.get(&Symbol::new("X")),
        Some(&Expr::Symbol(Symbol::new("bob")))
    );

    let mut rejected = LogicEnv::new();
    let different_subject = Expr::List(vec![
        Expr::Symbol(Symbol::new("same")),
        Expr::Symbol(Symbol::new("bob")),
        Expr::Symbol(Symbol::new("alice")),
    ]);
    assert!(
        !rejected
            .unify(&mut cx, &pattern, &different_subject, OccursCheck::Always)
            .unwrap()
    );
    assert_eq!(rejected.get(&Symbol::new("X")), None);
}

#[test]
fn unify_returns_false_on_mismatch() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let mut env = LogicEnv::new();
    let accepted = env
        .unify(
            &mut cx,
            &Expr::Symbol(Symbol::new("a")),
            &Expr::Symbol(Symbol::new("b")),
            OccursCheck::Always,
        )
        .unwrap();
    assert!(!accepted);
}

fn live_shape_symbol() -> Symbol {
    Symbol::qualified("test", "live-shape")
}

fn live_shape_capability() -> CapabilityName {
    CapabilityName::new("test.live-shape")
}

struct RequiresCapabilityShape;

impl Shape for RequiresCapabilityShape {
    fn check_value(&self, cx: &mut Cx, value: sim_kernel::Value) -> Result<ShapeMatch> {
        let expr = value.object().as_expr(cx)?;
        self.check_expr(cx, &expr)
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        cx.require(&live_shape_capability())?;
        Ok(if *expr == Expr::Bool(true) {
            ShapeMatch::accept(MatchScore::exact(100))
        } else {
            ShapeMatch::reject("expected true")
        })
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("requires-capability"))
    }
}

struct RequiresCapabilityShapeLib;

impl Lib for RequiresCapabilityShapeLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("sim", "live-shape-test"),
            version: Version("0.1.0".to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Shape {
                symbol: live_shape_symbol(),
                shape_id: None,
            }],
        }
    }

    fn load(&self, _cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.shape_value(
            live_shape_symbol(),
            shape_value(live_shape_symbol(), Arc::new(RequiresCapabilityShape)),
        )?;
        Ok(())
    }
}

#[test]
fn shape_unify_uses_caller_context_for_registered_shapes() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    cx.load_lib(&RequiresCapabilityShapeLib).unwrap();

    let pattern = Expr::Symbol(live_shape_symbol());
    let subject = Expr::Bool(true);

    let denied = unify_exprs(&mut cx, &LogicConfig::default(), &pattern, &subject).unwrap_err();
    assert!(
        denied
            .to_string()
            .contains(live_shape_capability().as_str())
    );

    cx.grant(live_shape_capability());
    let matched = unify_exprs(&mut cx, &LogicConfig::default(), &pattern, &subject).unwrap();
    assert!(matched.accepted);
}
