use sim_kernel::{Cx, Expr, NumberLiteral, Symbol, Value};

use crate::{LuaEnv, LuaEvalPolicy, lua_coroutine_frame_value};

use sim_kernel::testing::bare_cx as cx;

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

fn int(cx: &mut Cx, value: i64) -> Value {
    cx.factory()
        .number_literal(Symbol::qualified("test", "i64"), value.to_string())
        .unwrap()
}

fn int_expr(cx: &mut Cx, value: i64) -> Expr {
    let value = int(cx, value);
    value_expr(cx, &value)
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

fn lua_call(operator: Expr, args: Vec<Expr>) -> Expr {
    let mut values = vec![operator];
    values.extend(args);
    lua_form("call", values)
}

fn lua_closure(
    name: &str,
    params: Vec<&str>,
    vararg: bool,
    body: Expr,
    captures: Vec<&str>,
) -> Expr {
    lua_form(
        "closure",
        vec![
            Expr::Symbol(Symbol::new(name)),
            Expr::List(
                params
                    .into_iter()
                    .map(|name| Expr::Symbol(Symbol::new(name)))
                    .collect(),
            ),
            Expr::Bool(vararg),
            body,
            Expr::List(
                captures
                    .into_iter()
                    .map(|name| Expr::Symbol(Symbol::new(name)))
                    .collect(),
            ),
        ],
    )
}

fn lua_return(value: Expr) -> Expr {
    lua_form("return", vec![value])
}

#[test]
fn lua_closure_captures_shared_upvalue_cell() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    env.define(Symbol::new("count"), int(&mut cx, 0)).unwrap();

    let body = lua_form(
        "block",
        vec![
            lua_assign(
                "count",
                lua_form(
                    "add",
                    vec![Expr::Local(Symbol::new("count")), int_expr(&mut cx, 1)],
                ),
            ),
            lua_return(Expr::Local(Symbol::new("count"))),
        ],
    );
    let closure = lua_closure("next", Vec::new(), false, body, vec!["count"]);
    let value = policy
        .eval(&mut cx, &mut env, &closure)
        .unwrap()
        .into_values()
        .remove(0);
    env.define(Symbol::new("next"), value).unwrap();

    for expected in ["1", "2", "3"] {
        let result = policy
            .eval(
                &mut cx,
                &mut env,
                &lua_call(Expr::Local(Symbol::new("next")), Vec::new()),
            )
            .unwrap();
        assert_eq!(
            number_canonical(&mut cx, &result.into_values()[0]),
            expected
        );
    }
}

#[test]
fn lua_multi_return_call_binds_multiple_locals() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();

    let returns_two = lua_closure(
        "returns_two",
        Vec::new(),
        false,
        lua_form(
            "return",
            vec![
                Expr::String("left".to_owned()),
                Expr::String("right".to_owned()),
            ],
        ),
        Vec::new(),
    );
    let program = lua_form(
        "chunk",
        vec![
            lua_local("f", Some(returns_two)),
            lua_form(
                "local-values",
                vec![
                    Expr::List(vec![
                        Expr::Symbol(Symbol::new("a")),
                        Expr::Symbol(Symbol::new("b")),
                    ]),
                    lua_call(Expr::Local(Symbol::new("f")), Vec::new()),
                ],
            ),
            lua_form(
                "return",
                vec![Expr::Local(Symbol::new("a")), Expr::Local(Symbol::new("b"))],
            ),
        ],
    );

    let values = policy
        .eval(&mut cx, &mut env, &program)
        .unwrap()
        .into_values();
    assert_eq!(
        value_expr(&mut cx, &values[0]),
        Expr::String("left".to_owned())
    );
    assert_eq!(
        value_expr(&mut cx, &values[1]),
        Expr::String("right".to_owned())
    );
}

#[test]
fn lua_varargs_and_select_use_lua_adjustment_rules() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    policy.install_stdlib(&mut cx, &mut env).unwrap();

    let arity = lua_closure(
        "arity",
        Vec::new(),
        true,
        lua_form(
            "return",
            vec![lua_call(
                Expr::Local(Symbol::new("select")),
                vec![
                    Expr::String("#".to_owned()),
                    lua_form("varargs", Vec::new()),
                ],
            )],
        ),
        Vec::new(),
    );
    let program = lua_form(
        "chunk",
        vec![
            lua_local("arity", Some(arity)),
            lua_return(lua_call(
                Expr::Local(Symbol::new("arity")),
                vec![
                    Expr::String("a".to_owned()),
                    Expr::String("b".to_owned()),
                    Expr::String("c".to_owned()),
                ],
            )),
        ],
    );

    let values = policy
        .eval(&mut cx, &mut env, &program)
        .unwrap()
        .into_values();
    assert_eq!(number_canonical(&mut cx, &values[0]), "3");
}

#[test]
fn lua_pcall_maps_lua_error_into_false_status_tuple() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    policy.install_stdlib(&mut cx, &mut env).unwrap();

    let boom = lua_closure(
        "boom",
        Vec::new(),
        false,
        lua_call(
            Expr::Local(Symbol::new("error")),
            vec![Expr::String("x".to_owned())],
        ),
        Vec::new(),
    );
    let program = lua_form(
        "chunk",
        vec![
            lua_local("boom", Some(boom)),
            lua_call(
                Expr::Local(Symbol::new("pcall")),
                vec![Expr::Local(Symbol::new("boom"))],
            ),
        ],
    );

    let values = policy
        .eval(&mut cx, &mut env, &program)
        .unwrap()
        .into_values();
    assert_eq!(value_expr(&mut cx, &values[0]), Expr::Bool(false));
    match value_expr(&mut cx, &values[1]) {
        Expr::String(message) => assert!(message.contains('x')),
        other => panic!("expected pcall error string, got {other:?}"),
    }
}

#[test]
fn lua_coroutine_stdlib_resumes_producer_consumer_frame() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    policy.install_stdlib(&mut cx, &mut env).unwrap();
    let produced = string(&mut cx, "produced");
    let consumed = string(&mut cx, "consumed");
    let thread = lua_coroutine_frame_value(&mut cx, vec![produced], vec![consumed]).unwrap();
    env.define(Symbol::new("co"), thread).unwrap();

    let first = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("coroutine.resume")),
                vec![Expr::Local(Symbol::new("co"))],
            ),
        )
        .unwrap()
        .into_values();
    assert_eq!(value_expr(&mut cx, &first[0]), Expr::Bool(true));
    assert_eq!(
        value_expr(&mut cx, &first[1]),
        Expr::String("produced".to_owned())
    );

    let second = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("coroutine.resume")),
                vec![Expr::Local(Symbol::new("co"))],
            ),
        )
        .unwrap()
        .into_values();
    assert_eq!(value_expr(&mut cx, &second[0]), Expr::Bool(true));
    assert_eq!(
        value_expr(&mut cx, &second[1]),
        Expr::String("consumed".to_owned())
    );

    let status = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("coroutine.status")),
                vec![Expr::Local(Symbol::new("co"))],
            ),
        )
        .unwrap()
        .into_values();
    assert_eq!(
        value_expr(&mut cx, &status[0]),
        Expr::String("dead".to_owned())
    );
}

#[test]
fn lua_coroutine_wrap_is_callable_and_preserves_multi_return() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    policy.install_stdlib(&mut cx, &mut env).unwrap();

    let worker = lua_closure(
        "worker",
        Vec::new(),
        false,
        lua_form(
            "return",
            vec![
                Expr::String("left".to_owned()),
                Expr::String("right".to_owned()),
            ],
        ),
        Vec::new(),
    );
    let program = lua_form(
        "chunk",
        vec![
            lua_local("worker", Some(worker)),
            lua_local(
                "wrapped",
                Some(lua_call(
                    Expr::Local(Symbol::new("coroutine.wrap")),
                    vec![Expr::Local(Symbol::new("worker"))],
                )),
            ),
            lua_return(lua_call(Expr::Local(Symbol::new("wrapped")), Vec::new())),
        ],
    );

    let values = policy
        .eval(&mut cx, &mut env, &program)
        .unwrap()
        .into_values();
    assert_eq!(
        value_expr(&mut cx, &values[0]),
        Expr::String("left".to_owned())
    );
    assert_eq!(
        value_expr(&mut cx, &values[1]),
        Expr::String("right".to_owned())
    );
}

#[test]
fn lua_numeric_for_loop_accumulates_through_outer_local() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    let program = lua_form(
        "chunk",
        vec![
            lua_local("sum", Some(int_expr(&mut cx, 0))),
            lua_form(
                "for-num",
                vec![
                    Expr::Symbol(Symbol::new("i")),
                    int_expr(&mut cx, 1),
                    int_expr(&mut cx, 3),
                    int_expr(&mut cx, 1),
                    lua_form(
                        "block",
                        vec![lua_assign(
                            "sum",
                            lua_form(
                                "add",
                                vec![
                                    Expr::Local(Symbol::new("sum")),
                                    Expr::Local(Symbol::new("i")),
                                ],
                            ),
                        )],
                    ),
                ],
            ),
            lua_return(Expr::Local(Symbol::new("sum"))),
        ],
    );

    let values = policy
        .eval(&mut cx, &mut env, &program)
        .unwrap()
        .into_values();
    assert_eq!(number_canonical(&mut cx, &values[0]), "6");
}
