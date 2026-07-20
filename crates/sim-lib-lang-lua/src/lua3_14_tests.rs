use sim_kernel::{CapabilityName, Cx, Error, Expr, NumberLiteral, Symbol, Value};

use crate::{LuaEnv, LuaEvalPolicy, load::eval_lua_source, lua_rawget, lua_table_value};

use sim_kernel::testing::bare_cx as cx;

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

fn eval_values(cx: &mut Cx, env: &mut LuaEnv, expr: Expr) -> Vec<Value> {
    let policy = LuaEvalPolicy::new(cx).unwrap();
    policy.eval(cx, env, &expr).unwrap().into_values()
}

fn assert_capability(err: Error, name: &str) {
    match err {
        Error::CapabilityDenied { capability } => {
            assert_eq!(capability, CapabilityName::new(name));
        }
        other => panic!("expected capability denial, got {other:?}"),
    }
}

#[test]
fn lua_load_decodes_text_chunks_and_executes_them() {
    let mut cx = cx();
    let values = eval_lua_source(&mut cx, "return load('return 1 + 2')()").unwrap();
    assert_eq!(number_canonical(&mut cx, &values[0]), "3");

    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    policy.install_stdlib(&mut cx, &mut env).unwrap();
    let err = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("load")),
                vec![
                    Expr::String("return 1".to_owned()),
                    Expr::String("chunk".to_owned()),
                    Expr::String("b".to_owned()),
                ],
            ),
        )
        .unwrap_err();
    assert!(format!("{err}").contains("bytecode"));
}

#[test]
fn lua_math_core_functions_follow_lua_number_subtypes() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    policy.install_stdlib(&mut cx, &mut env).unwrap();

    let floor = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("math.floor")),
                vec![Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("test", "f64"),
                    canonical: "4.8".to_owned(),
                })],
            ),
        )
        .unwrap()
        .into_values();
    assert_eq!(number_canonical(&mut cx, &floor[0]), "4");

    let sqrt = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("math.sqrt")),
                vec![Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("test", "i64"),
                    canonical: "9".to_owned(),
                })],
            ),
        )
        .unwrap()
        .into_values();
    assert_eq!(number_canonical(&mut cx, &sqrt[0]), "3.0");

    let max = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("math.max")),
                vec![
                    Expr::Number(NumberLiteral {
                        domain: Symbol::qualified("test", "i64"),
                        canonical: "2".to_owned(),
                    }),
                    Expr::Number(NumberLiteral {
                        domain: Symbol::qualified("test", "i64"),
                        canonical: "7".to_owned(),
                    }),
                ],
            ),
        )
        .unwrap()
        .into_values();
    assert_eq!(number_canonical(&mut cx, &max[0]), "7");

    let ty = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("math.type")),
                vec![Expr::Number(NumberLiteral {
                    domain: Symbol::qualified("test", "i64"),
                    canonical: "7".to_owned(),
                })],
            ),
        )
        .unwrap()
        .into_values();
    assert_eq!(string_value(&mut cx, &ty[0]), "integer");
}

#[test]
fn lua_io_and_os_host_effects_fail_closed_without_capabilities() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    policy.install_stdlib(&mut cx, &mut env).unwrap();

    let io_err = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("io.open")),
                vec![
                    Expr::String("fixture.txt".to_owned()),
                    Expr::String("r".to_owned()),
                ],
            ),
        )
        .unwrap_err();
    assert_capability(io_err, "fs/read");

    let exec_err = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("os.execute")),
                vec![Expr::String("true".to_owned())],
            ),
        )
        .unwrap_err();
    assert_capability(exec_err, "exec");

    let getenv_err = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("os.getenv")),
                vec![Expr::String("HOME".to_owned())],
            ),
        )
        .unwrap_err();
    assert_capability(getenv_err, "env/read");
}

#[test]
fn lua_debug_safe_subset_reports_tracebacks_and_explicit_gaps() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    policy.install_stdlib(&mut cx, &mut env).unwrap();

    let traceback = eval_values(
        &mut cx,
        &mut env,
        lua_call(
            Expr::Local(Symbol::new("debug.traceback")),
            vec![Expr::String("boom".to_owned())],
        ),
    );
    let text = string_value(&mut cx, &traceback[0]);
    assert!(text.contains("boom"));
    assert!(text.contains("SIM Lua frame"));

    let sethook = eval_values(
        &mut cx,
        &mut env,
        lua_call(Expr::Local(Symbol::new("debug.sethook")), Vec::new()),
    );
    let kind_key = cx.factory().string("kind".to_owned()).unwrap();
    let kind = lua_rawget(&mut cx, &sethook[0], &kind_key)
        .unwrap()
        .unwrap();
    assert_eq!(string_value(&mut cx, &kind), "ExpectedGap");
}

#[test]
fn lua_package_table_exposes_searcher_order() {
    let mut cx = cx();
    let policy = LuaEvalPolicy::new(&mut cx).unwrap();
    let mut env = LuaEnv::new();
    policy.install_stdlib(&mut cx, &mut env).unwrap();

    let package = env.get(&Symbol::new("package")).unwrap();
    let searchers_key = cx.factory().string("searchers".to_owned()).unwrap();
    let searchers = lua_rawget(&mut cx, &package, &searchers_key)
        .unwrap()
        .unwrap();
    assert_eq!(
        lua_table_value(&searchers)
            .unwrap()
            .len_border(&mut cx)
            .unwrap(),
        4
    );

    let require_missing = policy
        .eval(
            &mut cx,
            &mut env,
            &lua_call(
                Expr::Local(Symbol::new("require")),
                vec![Expr::String("missing.module".to_owned())],
            ),
        )
        .unwrap_err();
    assert!(format!("{require_missing}").contains("module 'missing.module' not found"));
}
