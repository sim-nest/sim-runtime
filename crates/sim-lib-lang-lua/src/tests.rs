use std::sync::Arc;

use sim_kernel::{
    Args, Cx, Error, Expr, NumberLiteral, Ref, Symbol, Value,
    control::{control_aborted_status, control_result_status},
};
use sim_lib_control::{CoroutineLane, CoroutineStep};
use sim_lib_standard_core::{ProfileFunction, ProfileRegistry};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

fn int(cx: &mut Cx, value: i64) -> Value {
    cx.factory()
        .number_literal(Symbol::qualified("test", "i64"), value.to_string())
        .unwrap()
}

fn float_text(cx: &mut Cx, value: &str) -> Value {
    cx.factory()
        .number_literal(Symbol::qualified("test", "f64"), value.to_owned())
        .unwrap()
}

fn value_expr(cx: &mut Cx, value: &Value) -> Expr {
    value.object().as_expr(cx).unwrap()
}

fn number_canonical(cx: &mut Cx, value: &Value) -> String {
    match value_expr(cx, value) {
        Expr::Number(NumberLiteral { canonical, .. }) => canonical,
        other => panic!("expected number, got {other:?}"),
    }
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
        table
            .set_symbol(&mut cx, Symbol::new("name"), denied)
            .unwrap_err(),
        Error::CapabilityDenied { .. }
    ));
    cx.grant(sim_lib_mutation::standard_mutate_capability());
    let new = string(&mut cx, "new");
    table.set_symbol(&mut cx, Symbol::new("name"), new).unwrap();
    assert_eq!(
        table
            .get_symbol(&mut cx, Symbol::new("name"))
            .unwrap()
            .unwrap()
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::String("new".to_owned())
    );
}

#[test]
fn lua_numeric_operators_follow_lua_subtypes_and_coercions() {
    let mut cx = cx();
    let mut env = LuaEnv::new();

    let left = int(&mut cx, 3);
    let right = int(&mut cx, 2);
    let floordiv = lua_binary(&mut cx, &mut env, LuaOp::FloorDiv, left, right).unwrap();
    assert_eq!(number_canonical(&mut cx, &floordiv), "1");

    let left = int(&mut cx, 7);
    let right = int(&mut cx, 3);
    let modulo = lua_binary(&mut cx, &mut env, LuaOp::Mod, left, right).unwrap();
    assert_eq!(number_canonical(&mut cx, &modulo), "1");

    let left = int(&mut cx, 1);
    let right = int(&mut cx, 2);
    let div = lua_binary(&mut cx, &mut env, LuaOp::FloatDiv, left, right).unwrap();
    assert_eq!(number_canonical(&mut cx, &div), "0.5");

    let left = int(&mut cx, 5);
    let right = int(&mut cx, 3);
    let bitand = lua_binary(&mut cx, &mut env, LuaOp::Band, left, right).unwrap();
    assert_eq!(number_canonical(&mut cx, &bitand), "1");

    let left = int(&mut cx, 2);
    let right = int(&mut cx, 10);
    let pow = lua_binary(&mut cx, &mut env, LuaOp::Pow, left, right).unwrap();
    assert_eq!(number_canonical(&mut cx, &pow), "1024.0");

    let left = string(&mut cx, "2");
    let right = string(&mut cx, "3");
    let coerced = lua_binary(&mut cx, &mut env, LuaOp::Mul, left, right).unwrap();
    assert_eq!(number_canonical(&mut cx, &coerced), "6");
}

#[test]
fn lua_tables_support_mixed_keys_len_and_raw_index_bypass() {
    let mut cx = cx();
    cx.grant(sim_lib_mutation::standard_mutate_capability());
    let one = string(&mut cx, "one");
    let two = string(&mut cx, "two");
    let three = string(&mut cx, "three");
    let named = string(&mut cx, "named");
    let key1 = int(&mut cx, 1);
    let key2 = int(&mut cx, 2);
    let key3 = int(&mut cx, 3);
    let name_key = string(&mut cx, "name");
    let table = lua_table_from_values(
        &mut cx,
        vec![
            (key1, one.clone()),
            (key2, two),
            (key3, three),
            (name_key, named.clone()),
        ],
    )
    .unwrap();

    let float_one = float_text(&mut cx, "1.0");
    let raw_float_one = lua_rawget(&mut cx, &table, &float_one).unwrap().unwrap();
    assert_eq!(
        value_expr(&mut cx, &raw_float_one),
        value_expr(&mut cx, &one)
    );
    let mut env = LuaEnv::new();
    let len = lua_len(&mut cx, &mut env, table.clone()).unwrap();
    assert_eq!(number_canonical(&mut cx, &len), "3");

    let fallback_key = string(&mut cx, "fallback");
    let inherited = string(&mut cx, "inherited");
    let parent = lua_table_from_values(&mut cx, vec![(fallback_key.clone(), inherited)]).unwrap();
    let index_key = string(&mut cx, "__index");
    let metatable = lua_table_from_values(&mut cx, vec![(index_key, parent)]).unwrap();
    lua_set_metatable(&mut cx, &table, metatable).unwrap();

    let fallback = string(&mut cx, "fallback");
    let inherited = lua_get(&mut cx, &table, &fallback).unwrap().unwrap();
    assert_eq!(
        value_expr(&mut cx, &inherited),
        Expr::String("inherited".to_owned())
    );
    assert!(lua_rawget(&mut cx, &table, &fallback).unwrap().is_none());
}

#[test]
fn lua_add_metamethod_can_add_vector_like_tables() {
    let mut cx = cx();
    cx.grant(sim_lib_mutation::standard_mutate_capability());
    let vector_add = cx
        .factory()
        .opaque(Arc::new(ProfileFunction::new(
            lua_profile_symbol(),
            sim_lib_dispatch::dispatch_organ_symbol(),
            Symbol::qualified("lua", "vector-add"),
            |cx, args: Args| {
                let values = args.into_vec();
                if values.len() != 2 {
                    return Err(Error::Eval("vector add expects two values".to_owned()));
                }
                let x_key = cx.factory().string("x".to_owned())?;
                let y_key = cx.factory().string("y".to_owned())?;
                let mut env = LuaEnv::new();
                let lx = lua_rawget(cx, &values[0], &x_key)?
                    .ok_or_else(|| Error::Eval("left vector missing x".to_owned()))?;
                let ly = lua_rawget(cx, &values[0], &y_key)?
                    .ok_or_else(|| Error::Eval("left vector missing y".to_owned()))?;
                let rx = lua_rawget(cx, &values[1], &x_key)?
                    .ok_or_else(|| Error::Eval("right vector missing x".to_owned()))?;
                let ry = lua_rawget(cx, &values[1], &y_key)?
                    .ok_or_else(|| Error::Eval("right vector missing y".to_owned()))?;
                let x = lua_binary(cx, &mut env, LuaOp::Add, lx, rx)?;
                let y = lua_binary(cx, &mut env, LuaOp::Add, ly, ry)?;
                lua_table_from_values(cx, vec![(x_key, x), (y_key, y)])
            },
        )))
        .unwrap();
    let add_key = string(&mut cx, "__add");
    let metatable = lua_table_from_values(&mut cx, vec![(add_key, vector_add)]).unwrap();
    let x_key = string(&mut cx, "x");
    let y_key = string(&mut cx, "y");
    let left_x = int(&mut cx, 2);
    let left_y = int(&mut cx, 4);
    let right_x = int(&mut cx, 3);
    let right_y = int(&mut cx, 5);
    let left = lua_table_from_values(
        &mut cx,
        vec![(x_key.clone(), left_x), (y_key.clone(), left_y)],
    )
    .unwrap();
    let right = lua_table_from_values(&mut cx, vec![(x_key, right_x), (y_key, right_y)]).unwrap();
    lua_set_metatable(&mut cx, &left, metatable).unwrap();

    let mut env = LuaEnv::new();
    let result = lua_binary(&mut cx, &mut env, LuaOp::Add, left, right).unwrap();
    let x_key = string(&mut cx, "x");
    let y_key = string(&mut cx, "y");
    let x = lua_rawget(&mut cx, &result, &x_key).unwrap().unwrap();
    let y = lua_rawget(&mut cx, &result, &y_key).unwrap().unwrap();
    assert_eq!(number_canonical(&mut cx, &x), "5");
    assert_eq!(number_canonical(&mut cx, &y), "9");
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
