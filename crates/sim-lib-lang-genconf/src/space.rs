//! Shape-backed expression spaces for generative conformance.

use std::sync::Arc;

use sim_kernel::{Cx, Expr, NumberLiteral, ShapeDoc, Symbol};
use sim_shape::{AnyShape, ExprKind, ExprKindShape, ListShape, OrShape, Shape};

use crate::seed::r7rs_seed_corpus;

/// A deterministic, sized enumerator of `Expr` graphs drawn from a
/// Shape-described space.
///
/// The grammar shape is the membership oracle: an expression is in the space
/// exactly when `grammar.check_expr` accepts it.
pub struct ExprSpace {
    grammar: Arc<dyn Shape>,
    seed: Vec<Expr>,
    atoms: Vec<Expr>,
    max_depth: usize,
}

impl ExprSpace {
    /// Builds the R7RS-small core expression space.
    ///
    /// The first space covers booleans, symbols, strings, integer literals, and
    /// list compounds over those atoms, bounded by `max_depth`.
    pub fn r7rs_core_space(max_depth: usize) -> Self {
        Self {
            grammar: r7rs_core_grammar(),
            seed: r7rs_seed_corpus(),
            atoms: r7rs_core_atoms(),
            max_depth,
        }
    }

    /// Builds the shared core expression space for codec-neutral round trips.
    ///
    /// This space uses the same grammar and atoms as [`ExprSpace::r7rs_core_space`],
    /// but keeps the seed corpus to forms that general expression readers decode
    /// back to the same `Expr` without syntax lowering.
    pub fn core_round_trip_space(max_depth: usize) -> Self {
        Self {
            grammar: r7rs_core_grammar(),
            seed: core_round_trip_seed_corpus(),
            atoms: r7rs_core_atoms(),
            max_depth,
        }
    }

    /// Returns the Shape grammar used as this space's membership oracle.
    pub fn grammar(&self) -> Arc<dyn Shape> {
        Arc::clone(&self.grammar)
    }

    /// Returns the curated seed corpus this space contains.
    pub fn seed_corpus(&self) -> Vec<Expr> {
        self.seed.clone()
    }

    /// Returns a browsable description of this space's membership grammar.
    pub fn describe_grammar(&self, cx: &mut Cx) -> sim_kernel::Result<ShapeDoc> {
        self.grammar.describe(cx)
    }

    /// Returns the maximum expression depth used by enumeration.
    pub fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// Returns true when `expr` is a member of this space.
    pub fn contains(&self, cx: &mut Cx, expr: &Expr) -> bool {
        matches!(self.grammar.check_expr(cx, expr), Ok(matched) if matched.accepted)
    }

    /// Deterministically enumerates distinct in-space expressions in size order.
    ///
    /// Enumeration is stable for the same `(space, budget)` input.
    pub fn enumerate(&self, budget: usize) -> Vec<Expr> {
        let mut out = Vec::new();
        for seed in &self.seed {
            push_unique(&mut out, seed.clone(), budget);
        }
        for atom in &self.atoms {
            push_unique(&mut out, atom.clone(), budget);
        }

        let mut depth = 1;
        while depth < self.max_depth && out.len() < budget {
            let frontier = out.clone();
            for head in &frontier {
                for tail in &self.atoms {
                    if out.len() >= budget {
                        break;
                    }
                    push_unique(
                        &mut out,
                        Expr::List(vec![head.clone(), tail.clone()]),
                        budget,
                    );
                }
            }
            depth += 1;
        }
        out.truncate(budget);
        out
    }
}

/// Builds the canonical R7RS-small core Shape grammar.
pub fn r7rs_core_grammar() -> Arc<dyn Shape> {
    Arc::new(OrShape::new(vec![
        Arc::new(ExprKindShape::new(ExprKind::Bool)),
        Arc::new(ExprKindShape::new(ExprKind::Symbol)),
        Arc::new(ExprKindShape::new(ExprKind::String)),
        Arc::new(ExprKindShape::new(ExprKind::Number)),
        Arc::new(ListShape::with_rest(Vec::new(), Arc::new(AnyShape))),
    ]))
}

fn push_unique(out: &mut Vec<Expr>, expr: Expr, budget: usize) {
    if out.len() < budget && !out.iter().any(|existing| existing.canonical_eq(&expr)) {
        out.push(expr);
    }
}

fn r7rs_core_atoms() -> Vec<Expr> {
    vec![
        Expr::Bool(true),
        Expr::Bool(false),
        Expr::Symbol(Symbol::new("answer")),
        Expr::String("sim".to_owned()),
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "i64"),
            canonical: "1".to_owned(),
        }),
    ]
}

fn core_round_trip_seed_corpus() -> Vec<Expr> {
    vec![
        Expr::Bool(true),
        Expr::Bool(false),
        Expr::Symbol(Symbol::new("answer")),
        Expr::String("sim".to_owned()),
    ]
}

#[cfg(test)]
mod tests {
    use sim_kernel::testing::bare_cx as cx;

    use super::*;

    #[test]
    fn r7rs_core_enumeration_is_stable_and_in_space() {
        let mut cx = cx();
        let space = ExprSpace::r7rs_core_space(3);
        let first = space.enumerate(64);
        let second = space.enumerate(64);

        assert_eq!(first, second);
        assert!(first.len() > 5);
        for expr in first {
            assert!(space.contains(&mut cx, &expr), "out-of-space: {expr:?}");
        }
    }

    #[test]
    fn grammar_description_is_browsable() {
        let mut cx = cx();
        let space = ExprSpace::r7rs_core_space(3);

        let doc = space.describe_grammar(&mut cx).unwrap();

        assert_eq!(doc.name, "or shape");
        assert!(doc.details.iter().any(|detail| detail == "expr-kind bool"));
        assert!(doc.details.iter().any(|detail| detail == "list shape"));
    }

    #[test]
    fn grammar_tests_enumerated_exprs_are_all_in_space() {
        let mut cx = cx();
        let space = ExprSpace::r7rs_core_space(3);

        for expr in space.enumerate(128) {
            assert!(space.contains(&mut cx, &expr), "out-of-space: {expr:?}");
        }
        for seed in space.seed_corpus() {
            assert!(
                space.contains(&mut cx, &seed),
                "seed not in space: {seed:?}"
            );
        }
    }

    #[test]
    fn round_trip_space_omits_lowering_sensitive_quote_seed() {
        let space = ExprSpace::core_round_trip_space(3);
        let seed = space.seed_corpus();

        assert!(seed.iter().any(|expr| matches!(expr, Expr::Bool(true))));
        assert!(!seed.iter().any(|expr| matches!(
            expr,
            Expr::List(items)
                if items.first() == Some(&Expr::Symbol(Symbol::new("quote")))
        )));
    }
}
