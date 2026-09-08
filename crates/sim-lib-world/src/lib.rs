//! Loadable read-only world projection and explanation product.
//!
//! The product receives already observed semantic facts from its caller. It can
//! project, compare, and explain those facts, but has no observation, process,
//! network, clock, random, mutation, or proof-execution port.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod command;
mod product;
mod provider;

pub use command::{WORLD_VERB, WorldCommandLib};
pub use product::{
    DISCLOSURE_CONCLUSION, DISCLOSURE_FACT, SOURCE_CONCLUSION, SOURCE_FACT, WorldError,
    WorldProduct, WorldProjection,
};

#[cfg(test)]
mod tests;
