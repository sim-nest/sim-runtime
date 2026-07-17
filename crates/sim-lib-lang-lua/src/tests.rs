use sim_kernel::{
    Cx, Error, Expr, Ref, Symbol, Table, Value,
    control::{control_aborted_status, control_result_status},
};
use sim_lib_control::{CoroutineLane, CoroutineStep};
use sim_lib_standard_core::ProfileRegistry;

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

#[test]
fn lua_coroutines_reuse_control_organ() {
    let mut coroutine = lua_coroutine(
        vec![Ref::Symbol(Symbol::qualified("lua", "a"))],
        vec![Ref::Symbol(Symbol::qualified("lua", "b"))],
    );

    assert_eq!(
        coroutine.resume(),
        CoroutineStep::Yielded {
            lane: CoroutineLane::First,
            value: Ref::Symbol(Symbol::qualified("lua", "a"))
        }
    );
    assert_eq!(
        coroutine.resume(),
        CoroutineStep::Yielded {
            lane: CoroutineLane::Second,
            value: Ref::Symbol(Symbol::qualified("lua", "b"))
        }
    );
    assert!(
        lua_core_profile()
            .organs
            .iter()
            .any(|organ| organ.organ == sim_lib_control::control_organ_symbol())
    );
}

#[test]
fn lua_tables_reuse_mutation_organ() {
    let mut cx = cx();
    let old = string(&mut cx, "old");
    let table_value = lua_table(&mut cx, vec![(Symbol::new("name"), old)]).unwrap();
    let table = lua_table_value(&table_value).unwrap();

    let denied = string(&mut cx, "denied");
    assert!(matches!(
        table.set(&mut cx, Symbol::new("name"), denied).unwrap_err(),
        Error::CapabilityDenied { .. }
    ));
    cx.grant(sim_lib_mutation::standard_mutate_capability());
    let new = string(&mut cx, "new");
    table.set(&mut cx, Symbol::new("name"), new).unwrap();
    assert_eq!(
        table
            .get(&mut cx, Symbol::new("name"))
            .unwrap()
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::String("new".to_owned())
    );
}

#[test]
fn lua_profile_publishes_honest_fidelity() {
    let mut cx = cx();
    let mut registry = ProfileRegistry::new();
    let profile = install_lua_core_profile(&mut cx, &mut registry).unwrap();

    assert_eq!(
        profile
            .fidelity_badges
            .iter()
            .map(|badge| badge.level)
            .min(),
        Some(0)
    );
    assert!(
        profile
            .backing_requirements
            .contains(&Symbol::qualified("sim", "mutation"))
    );
    assert!(registry.profile(&profile.symbol).is_some());
    let result = sim_kernel::control::aborted_control_result(
        &mut cx,
        Ref::Symbol(Symbol::qualified("lua", "co")),
        Ref::Symbol(Symbol::qualified("lua", "yielded")),
    )
    .unwrap();
    assert_eq!(
        control_result_status(&cx, &result).unwrap(),
        Some(control_aborted_status())
    );
}
