// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::WorldError;
use crate::assay::{GraphMutation, SemanticBatch, judge, refuse_undeclared_read};
use crate::assay_oracle;

pub(crate) fn ensure_mutants_diverge(batch: &SemanticBatch) -> Result<(), WorldError> {
    for declaration in 1..=40 {
        let mutant = judge(batch, Some(GraphMutation::Declaration(declaration)))?;
        if assay_oracle::matches(&mutant) {
            return Err(WorldError::Qualification(format!(
                "declaration mutant {declaration} matched the independent oracle"
            )));
        }
    }
    for declaration in 1..=40 {
        let edges = 1 + usize::from(declaration <= 26);
        for edge in 0..edges {
            let mutant = judge(batch, Some(GraphMutation::Edge(declaration, edge)))?;
            if assay_oracle::matches(&mutant) {
                return Err(WorldError::Qualification(format!(
                    "edge mutant {declaration}:{edge} matched the independent oracle"
                )));
            }
        }
    }
    if refuse_undeclared_read(
        "conclusion/40",
        &["stable/40", "change/private-implementation"],
    )
    .is_ok()
    {
        return Err(WorldError::Qualification(
            "hidden dependency was admitted".to_owned(),
        ));
    }
    if refuse_undeclared_read(
        "conclusion/01",
        &["stable/01", "change/private-implementation"],
    )
    .is_err()
    {
        return Err(WorldError::Qualification(
            "declared read was refused".to_owned(),
        ));
    }
    Ok(())
}
