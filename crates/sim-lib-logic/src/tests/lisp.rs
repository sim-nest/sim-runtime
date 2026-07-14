use std::sync::Arc;

use sim_kernel::{CapabilitySet, Cx, DefaultFactory, EagerPolicy, ReadPolicy, Symbol};

use crate::{LogicLib, install_logic_lib, logic_db_write_capability};

#[test]
fn install_logic_lib_registers_surface_and_assert_query_work() {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    install_logic_lib(&mut cx).unwrap();
    cx.grant(logic_db_write_capability());
    let assert_fn = cx
        .resolve_function(&Symbol::qualified("logic", "assert!"))
        .unwrap();
    let query_fn = cx
        .resolve_function(&Symbol::qualified("logic", "query/all"))
        .unwrap();
    cx.call_exprs(
        assert_fn,
        vec![sim_kernel::Expr::Quote {
            mode: sim_kernel::QuoteMode::Quote,
            expr: Box::new(sim_kernel::Expr::List(vec![
                sim_kernel::Expr::Symbol(Symbol::new("fact")),
                sim_kernel::Expr::List(vec![
                    sim_kernel::Expr::Symbol(Symbol::new("parent")),
                    sim_kernel::Expr::Symbol(Symbol::new("alice")),
                    sim_kernel::Expr::Symbol(Symbol::new("bob")),
                ]),
            ])),
        }],
    )
    .unwrap();
    let answers = cx
        .call_exprs(
            query_fn,
            vec![sim_kernel::Expr::Quote {
                mode: sim_kernel::QuoteMode::Quote,
                expr: Box::new(sim_kernel::Expr::List(vec![
                    sim_kernel::Expr::Symbol(Symbol::new("parent")),
                    sim_kernel::Expr::Symbol(Symbol::new("alice")),
                    sim_kernel::Expr::Local(Symbol::new("x")),
                ])),
            }],
        )
        .unwrap();
    let expr = answers.object().as_expr(&mut cx).unwrap();
    assert!(matches!(expr, sim_kernel::Expr::List(_)));
    let _ = LogicLib;
    let _ = ReadPolicy {
        trust: sim_kernel::TrustLevel::TrustedSource,
        capabilities: CapabilitySet::default(),
    };
}
