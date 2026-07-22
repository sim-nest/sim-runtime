use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, ShapeMatch, Symbol};

use crate::{
    LogicConfig, LogicDb,
    builtins::{BuiltinBinding, BuiltinTable},
    query::{query_all, query_all_with_builtins},
    unify::occurs_check,
};

fn capture<'a>(answer: &'a ShapeMatch, name: &str) -> &'a Expr {
    answer
        .captures
        .exprs()
        .iter()
        .find_map(|(symbol, expr)| (symbol == &Symbol::new(name)).then_some(expr))
        .unwrap()
}

fn number(text: &str) -> Expr {
    Expr::Number(sim_kernel::NumberLiteral {
        domain: Symbol::qualified("numbers", "i64"),
        canonical: text.to_owned(),
    })
}

#[test]
fn standard_table_lists_is_and_findall_with_organs() {
    let table = BuiltinTable::standard();
    let keys = table.keys().cloned().collect::<Vec<_>>();

    assert!(keys.contains(&Symbol::new("is")));
    assert!(keys.contains(&Symbol::new("findall")));
    for key in ["member", "append", "length", "select"] {
        assert!(keys.contains(&Symbol::new(key)));
        assert_eq!(
            table.organ_of(&Symbol::new(key)),
            Some(&Symbol::new("sequence"))
        );
    }
    for key in ["#=", "#<", "dif"] {
        assert!(keys.contains(&Symbol::new(key)));
        assert_eq!(
            table.organ_of(&Symbol::new(key)),
            Some(&Symbol::new("control"))
        );
    }
    assert_eq!(
        table.organ_of(&Symbol::new("is")),
        Some(&Symbol::qualified("numbers", "arith"))
    );
    assert_eq!(
        table.organ_of(&Symbol::new("findall")),
        Some(&Symbol::new("sequence"))
    );
}

#[test]
fn standard_table_routes_existing_constraint_bindings() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let answers = query_all(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("between")),
            number("1"),
            number("2"),
            Expr::Local(Symbol::new("X")),
        ]),
        Some(10),
    )
    .unwrap();

    assert_eq!(answers.len(), 2);
}

#[test]
fn registering_a_new_key_resolves_without_resolver_change() {
    let mut table = BuiltinTable::standard();
    table.register(BuiltinBinding {
        key: Symbol::new("bind-ok"),
        organ: Symbol::qualified("test", "binding"),
        solve: Arc::new(|cx, ctx, args, env| {
            let [out] = args else {
                return Err(sim_kernel::Error::Eval(
                    "bind-ok expects one argument".to_owned(),
                ));
            };
            let mut next = env.clone();
            if next.unify(
                cx,
                out,
                &Expr::Symbol(Symbol::new("ok")),
                occurs_check(ctx.config),
            )? {
                Ok(vec![next])
            } else {
                Ok(Vec::new())
            }
        }),
    });

    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let answers = query_all_with_builtins(
        &mut cx,
        &LogicDb::new(),
        &LogicConfig::default(),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("bind-ok")),
            Expr::Local(Symbol::new("X")),
        ]),
        Some(1),
        table,
    )
    .unwrap();

    assert_eq!(answers.len(), 1);
    assert_eq!(capture(&answers[0], "X"), &Expr::Symbol(Symbol::new("ok")));
}
