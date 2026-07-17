use std::sync::Arc;

use sim_kernel::{
    Args, Cx, Error, Expr, NumberLiteral, Ref, Result, Symbol,
    card::{card_for_ref, card_kind_predicate},
    force_list_to_vec,
    standard::standard_organ_kind,
};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn number(cx: &mut Cx, value: u64) -> sim_kernel::Value {
    cx.factory()
        .number_literal(Symbol::qualified("test", "u64"), value.to_string())
        .unwrap()
}

fn bool_value(cx: &mut Cx, value: bool) -> sim_kernel::Value {
    cx.factory().bool(value).unwrap()
}

fn symbol_value(cx: &mut Cx, name: &str) -> sim_kernel::Value {
    cx.factory().symbol(Symbol::new(name)).unwrap()
}

fn number_from_value(cx: &mut Cx, value: &sim_kernel::Value) -> u64 {
    let Expr::Number(NumberLiteral { canonical, .. }) = value.object().as_expr(cx).unwrap() else {
        panic!("expected number value");
    };
    canonical.parse().unwrap()
}

fn bool_from_value(cx: &mut Cx, value: &sim_kernel::Value) -> bool {
    let Expr::Bool(value) = value.object().as_expr(cx).unwrap() else {
        panic!("expected bool value");
    };
    value
}

fn call_value(
    cx: &mut Cx,
    function: sim_kernel::Value,
    args: Vec<sim_kernel::Value>,
) -> Result<sim_kernel::Value> {
    let Some(callable) = function.object().as_callable() else {
        return Err(Error::TypeMismatch {
            expected: "callable binding value",
            found: "non-callable",
        });
    };
    callable.call(cx, Args::new(args))
}

#[test]
fn lexical_let_star_sees_prior_bindings() {
    let mut cx = cx();
    let env = LexicalEnv::new();
    let x = Symbol::new("x");
    let y = Symbol::new("y");

    let result = eval_let_star(
        &mut cx,
        &env,
        vec![
            {
                let x = x.clone();
                (
                    x,
                    Box::new(|cx: &mut Cx, _env: &LexicalEnv| Ok(number(cx, 2)))
                        as BindingInitializer,
                )
            },
            {
                let x = x.clone();
                let y = y.clone();
                (
                    y,
                    Box::new(move |cx: &mut Cx, env: &LexicalEnv| {
                        let left = number_from_value(cx, &env.lookup(&x)?);
                        Ok(number(cx, left + 3))
                    }) as BindingInitializer,
                )
            },
        ],
        |_cx, env| env.lookup(&y),
    )
    .unwrap();

    assert_eq!(number_from_value(&mut cx, &result), 5);
}

#[test]
fn letrec_handles_mutual_recursion() {
    let mut cx = cx();
    let root = LexicalEnv::new();
    let even = Symbol::new("even?");
    let odd = Symbol::new("odd?");

    let result = eval_letrec(
        &mut cx,
        &root,
        vec![
            {
                let even = even.clone();
                let odd = odd.clone();
                (
                    even.clone(),
                    Box::new(move |cx: &mut Cx, env: &LexicalEnv| {
                        let captured = env.clone();
                        let name = even.clone();
                        let peer = odd.clone();
                        lexical_function_value(
                            cx,
                            name,
                            captured,
                            Arc::new(move |cx, env, args| {
                                let n = number_from_value(cx, &args[0]);
                                if n == 0 {
                                    return Ok(bool_value(cx, true));
                                }
                                let next = number(cx, n - 1);
                                let peer_function = env.lookup(&peer)?;
                                call_value(cx, peer_function, vec![next])
                            }),
                        )
                    }) as BindingInitializer,
                )
            },
            {
                let even = even.clone();
                let odd = odd.clone();
                (
                    odd.clone(),
                    Box::new(move |cx: &mut Cx, env: &LexicalEnv| {
                        let captured = env.clone();
                        let name = odd.clone();
                        let peer = even.clone();
                        lexical_function_value(
                            cx,
                            name,
                            captured,
                            Arc::new(move |cx, env, args| {
                                let n = number_from_value(cx, &args[0]);
                                if n == 0 {
                                    return Ok(bool_value(cx, false));
                                }
                                let next = number(cx, n - 1);
                                let peer_function = env.lookup(&peer)?;
                                call_value(cx, peer_function, vec![next])
                            }),
                        )
                    }) as BindingInitializer,
                )
            },
        ],
        |cx, env| {
            let arg = number(cx, 8);
            let function = env.lookup(&even)?;
            call_value(cx, function, vec![arg])
        },
    )
    .unwrap();

    assert!(bool_from_value(&mut cx, &result));
}

#[test]
fn dynamic_binding_is_restored_after_escape() {
    let mut cx = cx();
    let env = DynamicEnv::new();
    let fluid = Symbol::new("fluid");

    let outer = symbol_value(&mut cx, "outer");
    let inner = symbol_value(&mut cx, "inner");
    let escaped = env.with_bindings(vec![(fluid.clone(), outer.clone())], || {
        assert_eq!(env.lookup(&fluid)?.unwrap(), outer);
        let result: Result<()> = env.with_bindings(vec![(fluid.clone(), inner)], || {
            assert!(env.lookup(&fluid)?.is_some());
            Err(Error::Eval("simulated non-local escape".to_owned()))
        });
        assert!(result.is_err());
        assert_eq!(env.lookup(&fluid)?.unwrap(), outer);
        Ok(())
    });

    escaped.unwrap();
    assert!(env.lookup(&fluid).unwrap().is_none());
}

#[test]
fn parameters_respect_control_dynamic_extent() {
    let mut cx = cx();
    let parameter = Parameter::new(
        Symbol::new("current-output"),
        symbol_value(&mut cx, "default"),
    );
    let temporary = symbol_value(&mut cx, "temporary");

    let result: Result<()> = parameter.with_value(temporary, || {
        assert_eq!(
            parameter.get()?.object().as_expr(&mut cx).unwrap(),
            Expr::Symbol(Symbol::new("temporary"))
        );
        Err(Error::Eval("simulated control escape".to_owned()))
    });

    assert!(result.is_err());
    assert_eq!(
        parameter.get().unwrap().object().as_expr(&mut cx).unwrap(),
        Expr::Symbol(Symbol::new("default"))
    );
}

#[test]
fn profile_options_select_binding_and_hygiene_modes() {
    let modes = BindingProfileModes::from_options(&[
        (
            Symbol::new("scope"),
            Expr::Symbol(Symbol::qualified("binding", "dynamic")),
        ),
        (
            Symbol::new("hygiene"),
            Expr::Symbol(Symbol::qualified("binding", "explicit")),
        ),
    ])
    .unwrap();

    assert_eq!(modes.scope, BindingScopeMode::Dynamic);
    assert_eq!(modes.hygiene, HygieneMode::Explicit);
    assert_eq!(
        BindingProfileModes::default().scope,
        BindingScopeMode::Lexical
    );
}

#[test]
fn binding_organ_claims_project_to_card() {
    let mut cx = cx();
    publish_binding_organ_claims(&mut cx).unwrap();

    let claims = cx
        .query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(binding_organ_symbol())),
            predicate: Some(card_kind_predicate()),
            object: Some(Ref::Symbol(standard_organ_kind())),
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(claims.len(), 1);

    let card = card_for_ref(&mut cx, Ref::Symbol(binding_organ_symbol())).unwrap();
    let table = card.object().as_table(&mut cx).unwrap();
    let entries = table.object().as_table_impl().unwrap();
    let ops = entries.get(&mut cx, Symbol::new("ops")).unwrap();
    let list = ops.object().as_list().unwrap();
    let values = force_list_to_vec(&mut cx, list, "binding organ ops").unwrap();

    assert_eq!(values.len(), 1);
    assert_eq!(
        values[0].object().as_expr(&mut cx).unwrap(),
        Expr::Symbol(Symbol::qualified("binding", "let.v1"))
    );
}

#[test]
fn binding_live_claims_match_loaded_exports() {
    let mut cx = cx();
    install_binding_lib(&mut cx).unwrap();
    let lib = cx.registry().lib(&manifest_name()).unwrap().clone();
    publish_binding_organ_claims_for_lib(&mut cx, lib.id).unwrap();

    let card = card_for_ref(&mut cx, Ref::Symbol(binding_organ_symbol())).unwrap();
    let table = card.object().as_table(&mut cx).unwrap();
    let entries = table.object().as_table_impl().unwrap();
    let ops = entries.get(&mut cx, Symbol::new("ops")).unwrap();
    let list = ops.object().as_list().unwrap();
    let card_ops = force_list_to_vec(&mut cx, list, "binding live ops")
        .unwrap()
        .into_iter()
        .map(|value| value.object().as_expr(&mut cx).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(card_ops.len(), binding_live_ops().len());

    for (op_key, export_symbol) in binding_live_ops() {
        let op_symbol = Symbol::qualified(
            op_key.namespace.to_string(),
            format!("{}.v{}", op_key.name, op_key.version),
        );
        assert!(
            card_ops.contains(&Expr::Symbol(op_symbol.clone())),
            "missing live binding claim {op_symbol}"
        );
        assert!(
            lib.exports
                .iter()
                .any(|export| export.symbol == export_symbol),
            "missing binding export {export_symbol}"
        );
        assert!(
            cx.resolve_function(&export_symbol).is_ok(),
            "{export_symbol}"
        );
    }
}

// ---- COOKBOOK_7 COOK7.02: the `let` binding organ (special form) ----

#[test]
fn let_special_form_binds_parallel_in_child_scope() {
    use sim_kernel::{DefaultFactory, EagerPolicy};

    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    install_binding_lib(&mut cx).unwrap();

    let sym = |name: &str| Expr::Symbol(Symbol::new(name));
    let clause = |name: &str, init: Expr| Expr::List(vec![sym(name), init]);
    let let_call = |bindings: Expr, body: Vec<Expr>| {
        let mut args = vec![bindings];
        args.extend(body);
        Expr::Call {
            operator: Box::new(sym("let")),
            args,
        }
    };
    let s = |text: &str| Expr::String(text.to_owned());

    // Single binding, body reads it back.
    let single = cx
        .eval_expr(let_call(
            Expr::List(vec![clause("x", s("five"))]),
            vec![sym("x")],
        ))
        .unwrap();
    assert_eq!(single.object().as_expr(&mut cx).unwrap(), s("five"));

    // Parallel bindings: both are visible in the body.
    let parallel = cx
        .eval_expr(let_call(
            Expr::List(vec![clause("x", s("a")), clause("y", s("b"))]),
            vec![sym("y")],
        ))
        .unwrap();
    assert_eq!(parallel.object().as_expr(&mut cx).unwrap(), s("b"));

    // The binding is scoped: an outer `x` is shadowed inside and restored after.
    let outer = cx.factory().string("outer".to_owned()).unwrap();
    cx.env_mut().define(Symbol::new("x"), outer);
    let shadowed = cx
        .eval_expr(let_call(
            Expr::List(vec![clause("x", s("inner"))]),
            vec![sym("x")],
        ))
        .unwrap();
    assert_eq!(shadowed.object().as_expr(&mut cx).unwrap(), s("inner"));
    assert_eq!(
        cx.env()
            .get(&Symbol::new("x"))
            .unwrap()
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        s("outer"),
        "outer binding must be restored after let"
    );

    // Applying `let` to already-evaluated args is a usage error (special form).
    let form = cx.resolve_function(&Symbol::new("let")).unwrap();
    let err = form
        .object()
        .as_callable()
        .unwrap()
        .call(&mut cx, Args::new(vec![]))
        .unwrap_err();
    assert!(matches!(err, Error::Eval(msg) if msg.contains("special form")));
}
