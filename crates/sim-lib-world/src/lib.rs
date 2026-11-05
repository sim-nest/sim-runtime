// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loadable read-only world projection and explanation product.
//!
//! The product receives already observed semantic facts from its caller. It can
//! project, compare, and explain those facts, but has no observation, process,
//! network, clock, random, mutation, or proof-execution port. The loaded command
//! reports that custody, refuses an unsupported pair or a malformed call,
//! measures resident cached why from one sealed snapshot, and reports
//! projection sensitivity from provider-read baseline and variant bytes.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod assay;
mod assay_mutant;
mod command;
mod grant;
mod product;
mod provider;
mod query;

pub use command::{ModelDomainLoad, WORLD_VERB, WorldCommandLib};
pub use product::{
    DISCLOSURE_CONCLUSION, DISCLOSURE_FACT, SOURCE_CONCLUSION, SOURCE_FACT, WorldError,
    WorldProduct, WorldProjection,
};

#[cfg(test)]
mod tests;
