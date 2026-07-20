use sim_kernel::{Cx, Expr, NumberLiteral, Symbol, Value};

use crate::{LuaEnv, LuaEvalPolicy, lua_rawget, lua_table_from_values};

use sim_kernel::testing::bare_cx as cx;

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

fn int(cx: &mut Cx, value: i64) -> Value {
    crate::lua_integer_value(cx, value).unwrap()
}

fn value_expr(cx: &mut Cx, value: &Value) -> Expr {
    value.object().as_expr(cx).unwrap()
}

fn string_value(cx: &mut Cx, value: &Value) -> String {
    match value_expr(cx, value) {
        Expr::String(text) => text,
        other => panic!("expected string, got {other:?}"),
    }
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

fn lua_call(operator: Expr, args: Vec<Expr>) -> Expr {
    let mut values = vec![operator];
    values.extend(args);
    lua_form("call", values)
}

fn lua_closure(name: &str, params: Vec<&str>, body: Expr) -> Expr {
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
            Expr::Bool(false),
            body,
            Expr::List(Vec::new()),
        ],
    )
}

fn lua_return(values: Vec<Expr>) -> Expr {
    lua_form("return", values)
}

#[test]
fn lua_string_gsub_and_match_use_pattern_vm() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    policy.install_stdlib(&mut cx, &mut env).unwrap();

    let gsub = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("string.gsub")),
                vec![
                    Expr::String("hello world".to_owned()),
                    Expr::String("%w+".to_owned()),
                    Expr::String("(%0)".to_owned()),
                ],
            ),
        )
        .unwrap()
        .into_values();
    assert_eq!(string_value(&mut cx, &gsub[0]), "(hello) (world)");
    assert_eq!(number_canonical(&mut cx, &gsub[1]), "2");

    let matched = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("string.match")),
                vec![
                    Expr::String("k=v".to_owned()),
                    Expr::String("(%w+)=(%w+)".to_owned()),
                ],
            ),
        )
        .unwrap()
        .into_values();
    assert_eq!(string_value(&mut cx, &matched[0]), "k");
    assert_eq!(string_value(&mut cx, &matched[1]), "v");
}

#[test]
fn lua_table_sort_accepts_comparator_and_unpack_returns_many() {
    let mut cx = cx();
    cx.grant(sim_lib_mutation::standard_mutate_capability());
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    policy.install_stdlib(&mut cx, &mut env).unwrap();

    let one_key = int(&mut cx, 1);
    let two_key = int(&mut cx, 2);
    let three_key = int(&mut cx, 3);
    let one = int(&mut cx, 1);
    let three = int(&mut cx, 3);
    let two = int(&mut cx, 2);
    let table = lua_table_from_values(
        &mut cx,
        vec![(one_key, one), (two_key, three), (three_key, two)],
    )
    .unwrap();
    env.define(Symbol::new("items"), table.clone()).unwrap();
    let desc = lua_closure(
        "desc",
        vec!["a", "b"],
        lua_return(vec![lua_form(
            "lt",
            vec![Expr::Local(Symbol::new("b")), Expr::Local(Symbol::new("a"))],
        )]),
    );
    let desc_value = policy
        .eval(&mut cx, &mut env, &desc)
        .unwrap()
        .into_values()
        .remove(0);
    env.define(Symbol::new("desc"), desc_value).unwrap();

    policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("table.sort")),
                vec![
                    Expr::Local(Symbol::new("items")),
                    Expr::Local(Symbol::new("desc")),
                ],
            ),
        )
        .unwrap();

    let first_key = int(&mut cx, 1);
    let second_key = int(&mut cx, 2);
    let third_key = int(&mut cx, 3);
    let first = lua_rawget(&mut cx, &table, &first_key).unwrap().unwrap();
    let second = lua_rawget(&mut cx, &table, &second_key).unwrap().unwrap();
    let third = lua_rawget(&mut cx, &table, &third_key).unwrap().unwrap();
    assert_eq!(number_canonical(&mut cx, &first), "3");
    assert_eq!(number_canonical(&mut cx, &second), "2");
    assert_eq!(number_canonical(&mut cx, &third), "1");

    let unpacked = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("table.unpack")),
                vec![Expr::Local(Symbol::new("items"))],
            ),
        )
        .unwrap()
        .into_values();
    assert_eq!(number_canonical(&mut cx, &unpacked[0]), "3");
    assert_eq!(number_canonical(&mut cx, &unpacked[1]), "2");
    assert_eq!(number_canonical(&mut cx, &unpacked[2]), "1");
}

#[test]
fn lua_utf8_len_and_string_dump_expected_gap_are_explicit() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    policy.install_stdlib(&mut cx, &mut env).unwrap();

    let utf8_len = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("utf8.len")),
                vec![Expr::String("\u{00e5}\u{00df}\u{6c34}".to_owned())],
            ),
        )
        .unwrap()
        .into_values();
    assert_eq!(number_canonical(&mut cx, &utf8_len[0]), "3");

    let dumped = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("string.dump")),
                vec![lua_closure("noop", Vec::new(), lua_return(Vec::new()))],
            ),
        )
        .unwrap()
        .into_values()
        .remove(0);
    let kind_key = string(&mut cx, "kind");
    let kind = lua_rawget(&mut cx, &dumped, &kind_key).unwrap().unwrap();
    assert_eq!(string_value(&mut cx, &kind), "ExpectedGap");
}
