// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::WorldError;
use crate::assay::{AssayObservation, GraphMutation, SemanticBatch, judge, refuse_undeclared_read};

pub(crate) fn ensure_mutants_diverge(
    batch: &SemanticBatch,
    sound: &AssayObservation,
) -> Result<(), WorldError> {
    for declaration in 1..=40 {
        let mutant = judge(batch, Some(GraphMutation::Declaration(declaration)))?;
        if same_prediction(&mutant, sound) {
            return Err(WorldError::Qualification(format!(
                "declaration mutant {declaration} matched the unmutated prediction"
            )));
        }
    }
    for declaration in 1..=40 {
        let edges = 1 + usize::from(declaration <= 26);
        for edge in 0..edges {
            let mutant = judge(batch, Some(GraphMutation::Edge(declaration, edge)))?;
            if same_prediction(&mutant, sound) {
                return Err(WorldError::Qualification(format!(
                    "edge mutant {declaration}:{edge} matched the unmutated prediction"
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

fn same_prediction(mutant: &AssayObservation, sound: &AssayObservation) -> bool {
    mutant.denominator == sound.denominator
        && mutant.rows.len() == sound.rows.len()
        && mutant.rows.iter().zip(&sound.rows).all(|(row, expected)| {
            row.changed == expected.changed
                && row.affected == expected.affected
                && row.revision_delta == expected.revision_delta
                && row.carried_state == expected.carried_state
        })
}
