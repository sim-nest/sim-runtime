//! Seed expression corpus for generated language conformance.

use sim_kernel::{Expr, Symbol};

/// Returns the readable R7RS-small seed corpus.
pub fn r7rs_seed_corpus() -> Vec<Expr> {
    vec![
        Expr::Bool(true),
        Expr::Bool(false),
        Expr::String("sim".to_owned()),
        Expr::Symbol(Symbol::new("answer")),
        Expr::List(vec![
            Expr::Symbol(Symbol::new("quote")),
            Expr::Symbol(Symbol::new("answer")),
        ]),
    ]
}
