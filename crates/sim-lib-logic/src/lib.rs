#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Logic behavior for the SIM runtime: clauses, unification, and queries.
//!
//! The kernel defines the `Shape`, eval-policy, and codec contracts; this crate
//! supplies the concrete logic organ (a clause database, unifier, constraint
//! solving, and a query/stream surface).

mod all_solutions;
mod arith;
pub mod builtins;
pub mod capabilities;
mod clause;
mod codec;
mod constraints;
mod cut;
mod db;
mod env;
mod error;
mod lisp;
mod lisp_runtime;
mod lists;
mod model;
mod naf;
pub mod policy;
mod query;
mod shapes;
mod stream;
mod unify;

pub use capabilities::{
    logic_consult_file_capability, logic_db_write_capability, logic_tool_call_capability,
};
pub use clause::{Clause, ClauseId, parse_clause_expr};
pub use db::LogicDb;
pub use env::LogicEnv;
pub use lisp::realize_logic;
pub use lisp::{LogicLib, install_logic_lib};
pub use model::{LogicConfig, LogicLimits, OccursCheck, SearchStrategy};
pub use policy::LogicPolicy;
pub use query::{LogicQuery, query, query_all, query_all_with_builtins};
pub use unify::unify_exprs;

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
