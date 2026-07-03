//! Clause database: the store of facts and rules a query resolves against.
//!
//! [`LogicDb`] owns the clauses and maintains predicate and first-argument
//! indexes so the resolver can narrow candidates quickly. It is plain runtime
//! state, not a kernel contract; see the [`README`](https://docs.rs/sim-runtime).

use std::collections::BTreeMap;

use sim_kernel::{Expr, Result, Symbol};

use crate::clause::{
    Clause, ClauseId, goal_arity, goal_first_arg, normalize_goal_expr, parse_clause_expr,
    predicate_symbol,
};

/// An indexed store of logic [`Clause`]s queried by goal.
///
/// Clauses are kept in assertion order and indexed by predicate symbol and by
/// first ground argument to speed candidate lookup.
#[derive(Clone, Debug, Default)]
pub struct LogicDb {
    clauses: Vec<Clause>,
    by_predicate: BTreeMap<Symbol, Vec<ClauseId>>,
    by_first_arg: BTreeMap<(Symbol, usize, String), Vec<ClauseId>>,
}

impl LogicDb {
    /// Creates an empty clause database.
    ///
    /// # Examples
    ///
    /// ```
    /// use sim_kernel::{Expr, Symbol};
    /// use sim_lib_logic::LogicDb;
    ///
    /// let mut db = LogicDb::new();
    /// db.assert_clause_expr(Expr::List(vec![
    ///     Expr::Symbol(Symbol::new("fact")),
    ///     Expr::List(vec![
    ///         Expr::Symbol(Symbol::new("parent")),
    ///         Expr::Symbol(Symbol::new("alice")),
    ///         Expr::Symbol(Symbol::new("bob")),
    ///     ]),
    /// ]))
    /// .unwrap();
    /// assert!(db.predicate_exists(&Symbol::new("parent")));
    /// assert_eq!(db.clauses().len(), 1);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns all stored clauses in assertion order.
    pub fn clauses(&self) -> &[Clause] {
        &self.clauses
    }

    /// Returns whether any clause defines the given predicate symbol.
    pub fn predicate_exists(&self, predicate: &Symbol) -> bool {
        self.by_predicate
            .get(predicate)
            .is_some_and(|clauses| !clauses.is_empty())
    }

    /// Parses a clause expression and asserts it, returning its new id.
    pub fn assert_clause_expr(&mut self, expr: Expr) -> Result<ClauseId> {
        let id = ClauseId(self.clauses.len() + 1);
        let clause = parse_clause_expr(id, expr)?;
        self.assert_clause(clause)
    }

    /// Adds a parsed clause to the database and updates the indexes.
    pub fn assert_clause(&mut self, clause: Clause) -> Result<ClauseId> {
        let id = clause.id;
        let predicate = clause.predicate()?;
        let arity = clause.arity()?;
        let first_key = goal_first_arg(&clause.head)
            .filter(|expr| !matches!(expr, Expr::Local(_)))
            .map(canonical_goal_key);
        self.by_predicate
            .entry(predicate.clone())
            .or_default()
            .push(id);
        if let Some(first_key) = first_key {
            self.by_first_arg
                .entry((predicate, arity, first_key))
                .or_default()
                .push(id);
        }
        self.clauses.push(clause);
        Ok(id)
    }

    /// Removes the first clause matching `expr`, rebuilding the indexes.
    ///
    /// Returns whether a matching clause was found and removed.
    pub fn retract_clause_expr(&mut self, expr: &Expr) -> Result<bool> {
        let target = parse_clause_expr(ClauseId(0), expr.clone())?;
        if let Some(index) = self
            .clauses
            .iter()
            .position(|clause| clause.fact_expr().canonical_eq(&target.fact_expr()))
        {
            self.clauses.remove(index);
            self.rebuild_indexes()?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Returns the surface expressions of every fact for the given predicate.
    pub fn facts(&self, predicate: &Symbol) -> Vec<Expr> {
        self.clauses
            .iter()
            .filter(|clause| clause.body.is_empty())
            .filter_map(|clause| match clause.predicate() {
                Ok(symbol) if &symbol == predicate => Some(clause.fact_expr()),
                _ => None,
            })
            .collect()
    }

    /// Looks up a clause by its identifier.
    pub fn clause_by_id(&self, id: ClauseId) -> Option<&Clause> {
        self.clauses.iter().find(|clause| clause.id == id)
    }

    /// Returns the candidate clauses that could resolve `goal`.
    ///
    /// When `indexing` is true the first-argument index is consulted before
    /// falling back to the predicate index.
    pub fn clauses_for_goal(&self, goal: &Expr, indexing: bool) -> Result<Vec<&Clause>> {
        let predicate = predicate_symbol(goal)?;
        let arity = goal_arity(goal)?;
        let ids = if indexing {
            let first_key = goal_first_arg(goal)
                .filter(|expr| !matches!(expr, Expr::Local(_)))
                .map(canonical_goal_key);
            if let Some(first_key) = first_key {
                self.by_first_arg
                    .get(&(predicate.clone(), arity, first_key))
                    .cloned()
                    .or_else(|| self.by_predicate.get(&predicate).cloned())
                    .unwrap_or_default()
            } else {
                self.by_predicate
                    .get(&predicate)
                    .cloned()
                    .unwrap_or_default()
            }
        } else {
            self.by_predicate
                .get(&predicate)
                .cloned()
                .unwrap_or_default()
        };
        Ok(ids
            .into_iter()
            .filter_map(|id| self.clause_by_id(id))
            .collect())
    }

    fn rebuild_indexes(&mut self) -> Result<()> {
        self.by_predicate.clear();
        self.by_first_arg.clear();
        let clauses = std::mem::take(&mut self.clauses);
        for clause in clauses {
            self.assert_clause(clause)?;
        }
        Ok(())
    }
}

fn canonical_goal_key(expr: &Expr) -> String {
    format!("{:?}", normalize_goal_expr(expr).canonical_key())
}
