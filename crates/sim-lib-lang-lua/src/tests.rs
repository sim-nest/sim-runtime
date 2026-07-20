use std::sync::Arc;

use sim_kernel::{
    Args, Cx, Error, Expr, Ref, Symbol, Table, Value,
    control::{control_aborted_status, control_result_status},
};
use sim_lib_control::{CoroutineLane, CoroutineStep};
use sim_lib_standard_core::{ProfileFunction, ProfileRegistry};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

fn lua_form(name: &str, args: Vec<Expr>) -> Expr {
    let mut items = vec![Expr::Symbol(Symbol::qualified("lua", name))];
    items.extend(args);
    Expr::List(items)
}

fn lua_local(name: &str, value: Option<Expr>) -> Expr {
    let mut args = vec![Expr::Symbol(Symbol::new(name))];
    if let Some(value) = value {
        args.push(value);
    }
    lua_form("local", args)
}

fn lua_assign(name: &str, value: Expr) -> Expr {
    lua_form("assign", vec![Expr::Symbol(Symbol::new(name)), value])
}

fn lua_return(value: Expr) -> Expr {
    lua_form("return", vec![value])
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
    let declared = lua_core_profile();
    let profile = install_lua_core_profile(&mut cx, &mut registry).unwrap();

    assert_eq!(profile.reader, Symbol::qualified("codec", "lua"));
    assert_eq!(profile.eval_policy, lua_eval_policy_symbol());
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
    assert!(
        profile
            .backing_requirements
            .contains(&Symbol::qualified("sim", "dispatch"))
    );
    for organ in [
        sim_lib_binding::binding_organ_symbol(),
        sim_lib_control::control_organ_symbol(),
        sim_lib_mutation::mutation_organ_symbol(),
        sim_lib_sequence::sequence_organ_symbol(),
        sim_lib_dispatch::dispatch_organ_symbol(),
    ] {
        assert!(
            declared.organs.iter().any(|use_| use_.organ == organ),
            "missing Lua organ {organ}"
        );
    }
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

#[test]
fn lua_truthiness_counts_only_nil_and_false_as_falsey() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let nil = cx.factory().nil().unwrap();
    let false_value = cx.factory().bool(false).unwrap();
    let true_value = cx.factory().bool(true).unwrap();
    let empty_string = string(&mut cx, "");

    assert!(!policy.kit().is_truthy(&mut cx, &nil).unwrap());
    assert!(!policy.kit().is_truthy(&mut cx, &false_value).unwrap());
    assert!(policy.kit().is_truthy(&mut cx, &true_value).unwrap());
    assert!(policy.kit().is_truthy(&mut cx, &empty_string).unwrap());
}

#[test]
fn lua_eval_policy_runs_chunk_with_core_forms() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    let choose =
        cx.factory()
            .opaque(Arc::new(ProfileFunction::new(
                lua_profile_symbol(),
                sim_lib_dispatch::dispatch_organ_symbol(),
                Symbol::qualified("lua", "choose"),
                |cx, args: Args| {
                    let first =
                        args.into_vec().into_iter().next().ok_or_else(|| {
                            Error::Eval("lua choose requires one value".to_owned())
                        })?;
                    match first.object().as_expr(cx)? {
                        Expr::String(value) => cx.factory().string(format!("{value}:called")),
                        _ => Err(Error::TypeMismatch {
                            expected: "string",
                            found: "non-string",
                        }),
                    }
                },
            )))
            .unwrap();
    env.define(Symbol::new("choose"), choose);

    let program = lua_form(
        "chunk",
        vec![
            lua_local("maybe", None),
            lua_local("value", Some(Expr::String("unset".to_owned()))),
            lua_form(
                "if",
                vec![
                    Expr::Local(Symbol::new("maybe")),
                    lua_assign("value", Expr::String("bad".to_owned())),
                    lua_assign(
                        "value",
                        lua_form(
                            "call",
                            vec![
                                Expr::Local(Symbol::new("choose")),
                                Expr::String("ok".to_owned()),
                            ],
                        ),
                    ),
                ],
            ),
            lua_return(Expr::Local(Symbol::new("value"))),
        ],
    );

    let result = policy.eval(&mut cx, &mut env, &program).unwrap();
    assert!(result.is_return());
    let values = result.into_values();
    assert_eq!(values.len(), 1);
    assert_eq!(
        values[0].object().as_expr(&mut cx).unwrap(),
        Expr::String("ok:called".to_owned())
    );
}
