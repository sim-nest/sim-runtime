use std::sync::Arc;

use sim_kernel::{
    CapabilityName, CapabilitySet, Cx, DefaultFactory, Demand, EagerPolicy, Error, EvalPolicy,
    Expr, PreparedArgs, RawArgs, Result, Symbol, Value, read_eval_capability,
};

use super::*;

fn cx() -> (Cx, sim_kernel::GrantSeat) {
    Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory))
}

fn key(name: &str) -> Expr {
    Expr::Symbol(Symbol::new(name.to_owned()))
}

fn field<'a>(expr: &'a Expr, name: &str) -> &'a Expr {
    let Expr::Map(entries) = expr else {
        panic!("expected map, got {expr:?}");
    };
    entries
        .iter()
        .find_map(|(key, value)| (key == &self::key(name)).then_some(value))
        .unwrap_or_else(|| panic!("missing key {name:?} in {entries:?}"))
}

fn text(value: &str) -> Expr {
    Expr::String(value.to_owned())
}

fn shape(name: &str) -> Expr {
    Expr::String(name.to_owned())
}

fn empty_caps() -> Expr {
    Expr::List(Vec::new())
}

fn caps(items: &[&str]) -> Expr {
    Expr::List(items.iter().copied().map(text).collect())
}

fn config_eval_entry(node: Expr) -> (Expr, Expr) {
    (key("config/eval"), node)
}

fn node_map_expr(expr: Expr, shape: &str) -> Expr {
    Expr::Map(vec![
        (key("codec"), text("codec/lisp")),
        (key("expr"), expr),
        (key("requires"), empty_caps()),
        (key("allow"), caps(&["read-eval"])),
        (key("shape"), self::shape(shape)),
    ])
}

fn node_list_expr(expr: Expr, shape: &str, allow: &[&str]) -> Expr {
    Expr::List(vec![
        Expr::Symbol(config_eval_node_symbol()),
        key(":codec"),
        text("codec/lisp"),
        key(":expr"),
        expr,
        key(":requires"),
        empty_caps(),
        key(":allow"),
        caps(allow),
        key(":shape"),
        self::shape(shape),
    ])
}

fn opt_in() -> HostConfigEvalOptIn {
    HostConfigEvalOptIn::trusted(CapabilitySet::new())
}

#[test]
fn config_without_node_is_unchanged() {
    let (mut cx, seat) = cx();
    seat.grant(&mut cx, read_eval_capability());
    let broker = ReadEvalBroker::new();
    let config = Expr::Map(vec![(key("enabled"), Expr::Bool(true))]);

    let realized = realize_config_expr(&mut cx, &broker, Some(&opt_in()), config.clone()).unwrap();

    assert_eq!(realized, config);
    assert!(broker.decisions(&cx).unwrap().is_empty());
}

#[test]
fn config_eval_node_without_host_opt_in_is_unchanged() {
    let (mut cx, _seat) = cx();
    let broker = ReadEvalBroker::new();
    let config = Expr::Map(vec![config_eval_entry(node_map_expr(
        Expr::Map(vec![(key("generated"), text("ok"))]),
        "Map",
    ))]);

    let realized = realize_config_expr(&mut cx, &broker, None, config.clone()).unwrap();

    assert_eq!(realized, config);
    assert!(broker.decisions(&cx).unwrap().is_empty());
}

#[test]
fn config_eval_shape_mismatch_is_denied() {
    let (mut cx, seat) = cx();
    seat.grant(&mut cx, read_eval_capability());
    let broker = ReadEvalBroker::new();
    let config = Expr::Map(vec![config_eval_entry(node_map_expr(
        text("not a map"),
        "Map",
    ))]);

    let err = realize_config_expr(&mut cx, &broker, Some(&opt_in()), config).unwrap_err();

    assert!(matches!(err, Error::WrongShape { .. }));
    let decisions = broker.decisions(&cx).unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].outcome, ReadEvalOutcome::ShapeDenied);
}

#[test]
fn config_eval_opt_in_merges_matching_map_result() {
    let (mut cx, seat) = cx();
    seat.grant(&mut cx, read_eval_capability());
    let broker = ReadEvalBroker::new();
    let config = Expr::Map(vec![
        (key("enabled"), Expr::Bool(true)),
        config_eval_entry(node_map_expr(
            Expr::Map(vec![(key("generated"), text("ok"))]),
            "Map",
        )),
    ]);

    let realized = realize_config_expr(&mut cx, &broker, Some(&opt_in()), config).unwrap();

    assert_eq!(field(&realized, "enabled"), &Expr::Bool(true));
    assert_eq!(field(&realized, "generated"), &text("ok"));
    let decisions = broker.decisions(&cx).unwrap();
    assert_eq!(decisions.len(), 1);
    assert_eq!(decisions[0].outcome, ReadEvalOutcome::Admitted);
}

struct ActiveCapabilityPolicy {
    capability: CapabilityName,
}

impl EvalPolicy for ActiveCapabilityPolicy {
    fn name(&self) -> &'static str {
        "config-eval-active-capability-probe"
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

fn capability_probe_cx(capability: CapabilityName) -> (Cx, sim_kernel::GrantSeat) {
    Cx::new_seated(
        Arc::new(ActiveCapabilityPolicy { capability }),
        Arc::new(DefaultFactory),
    )
}

fn realized_probe(capability: CapabilityName, grant_read_eval: bool) -> Expr {
    let (mut cx, seat) = capability_probe_cx(capability);
    if grant_read_eval {
        seat.grant(&mut cx, read_eval_capability());
    }
    let broker = ReadEvalBroker::new();
    let config = Expr::Map(vec![(
        key("probe"),
        node_list_expr(Expr::Nil, "Bool", &["read-eval", "secret/env"]),
    )]);

    realize_config_expr(&mut cx, &broker, Some(&opt_in()), config).unwrap()
}

#[test]
fn config_eval_node_cannot_self_grant_active_authority() {
    let secret = CapabilityName::new("secret/env");
    let secret_probe = realized_probe(secret, true);
    assert_eq!(field(&secret_probe, "probe"), &Expr::Bool(false));

    let read_eval_probe = realized_probe(read_eval_capability(), false);
    assert_eq!(field(&read_eval_probe, "probe"), &Expr::Bool(false));
}
