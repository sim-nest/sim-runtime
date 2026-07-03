use sim_kernel::{Datum, Expr, LocatedExprTree, Origin, Result, Symbol, Term};

use crate::symbols::scheme_symbol;

/// Result of lowering a Scheme `Expr` to a runtime value.
///
/// Evaluable forms lower to a [`Term`]; quoted or self-evaluating literals
/// lower to a [`Datum`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SchemeLowered {
    /// An evaluable term.
    Term(Term),
    /// A datum (quoted or self-evaluating literal).
    Datum(Datum),
}

/// A [`SchemeLowered`] value paired with its source origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedSchemeLowering {
    /// The lowered term or datum.
    pub lowered: SchemeLowered,
    /// Source origin carried from the located tree, if any.
    pub origin: Option<Origin>,
}

/// Lowers a located Scheme tree, preserving its origin.
pub fn lower_scheme_tree(tree: &LocatedExprTree) -> Result<LocatedSchemeLowering> {
    Ok(LocatedSchemeLowering {
        lowered: lower_scheme_expr(&tree.expr)?,
        origin: tree.origin.clone(),
    })
}

/// Lowers a single Scheme `Expr` to a [`SchemeLowered`] term or datum.
///
/// `quote` forms and self-evaluating literals become data; symbols, calls, and
/// blocks become evaluable terms over the canonical `scheme`-qualified surface.
pub fn lower_scheme_expr(expr: &Expr) -> Result<SchemeLowered> {
    match expr {
        Expr::Nil | Expr::Bool(_) | Expr::Number(_) | Expr::String(_) | Expr::Bytes(_) => {
            Datum::try_from(expr.clone()).map(SchemeLowered::Datum)
        }
        Expr::List(items) => lower_scheme_list(items),
        Expr::Symbol(_) | Expr::Call { .. } | Expr::Block(_) => {
            Term::lower(canonical_eval_expr(expr.clone())).map(SchemeLowered::Term)
        }
        _ => Term::lower(expr.clone()).map(SchemeLowered::Term),
    }
}

fn lower_scheme_list(items: &[Expr]) -> Result<SchemeLowered> {
    let Some(Expr::Symbol(head)) = items.first() else {
        return Datum::try_from(Expr::List(items.to_vec())).map(SchemeLowered::Datum);
    };
    if head == &Symbol::new("quote") {
        let [_, datum] = items else {
            return Err(sim_kernel::Error::Eval(
                "Scheme quote expects exactly one datum".to_owned(),
            ));
        };
        return Datum::try_from(datum.clone()).map(SchemeLowered::Datum);
    }
    Term::lower(canonical_call(head.clone(), &items[1..])).map(SchemeLowered::Term)
}

fn canonical_eval_expr(expr: Expr) -> Expr {
    match expr {
        Expr::List(items) => match items.first() {
            Some(Expr::Symbol(head)) => canonical_call(head.clone(), &items[1..]),
            _ => Expr::List(items),
        },
        other => other,
    }
}

fn canonical_call(head: Symbol, args: &[Expr]) -> Expr {
    if head == Symbol::new("begin") {
        return Expr::Block(args.iter().cloned().map(canonical_eval_expr).collect());
    }
    Expr::Call {
        operator: Box::new(Expr::Symbol(scheme_symbol(&head.to_string()))),
        args: args.iter().cloned().map(canonical_eval_expr).collect(),
    }
}
