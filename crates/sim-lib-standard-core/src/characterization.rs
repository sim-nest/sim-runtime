//! Landed evidence inventory for behavior characterization.
//!
//! This module records the typed owners that a characterization scenario can
//! compose. It is intentionally not a capture format: scenario execution and
//! capture identity remain extensions of [`crate::ConformanceHarness`].

use sim_kernel::Symbol;

use crate::{SourceConformanceCase, SourceConformanceCaseKind, SourceExpectation};

/// A stable evidence lane already available to a conformance case.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceLane {
    /// The scenario's declared inputs.
    Input,
    /// A successful result value.
    Value,
    /// A failed result.
    Failure,
    /// Ordered runtime events.
    Event,
    /// Ordered operation or library receipts.
    Receipt,
    /// The browseable Card face.
    Browse,
    /// Conformance outcome and fidelity evidence.
    Conformance,
}

/// Existing owner and canonical projection for an evidence lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvidenceLaneInventory {
    /// Stable lane.
    pub lane: EvidenceLane,
    /// Crate or product contract that owns the typed source record.
    pub owner: &'static str,
    /// Semantic projection used by a future capture.
    pub canonical_projection: &'static str,
}

/// Every stable typed evidence lane available to characterization scenarios.
///
/// No entry admits `Debug` text. Values without a canonical `Datum` face need
/// an explicit profile projection; failures similarly retain their typed
/// class, detail, and location rather than an error's host formatting.
pub const EVIDENCE_LANE_INVENTORY: &[EvidenceLaneInventory] = &[
    EvidenceLaneInventory {
        lane: EvidenceLane::Input,
        owner: "sim-lib-standard-core scenario contract",
        canonical_projection: "ordered declared input Datum records",
    },
    EvidenceLaneInventory {
        lane: EvidenceLane::Value,
        owner: "sim-kernel Value and DatumStore",
        canonical_projection: "canonical Datum or named profile projection",
    },
    EvidenceLaneInventory {
        lane: EvidenceLane::Failure,
        owner: "sim-kernel Error, Diagnostic, and Origin",
        canonical_projection: "stable failure class, detail, and source location",
    },
    EvidenceLaneInventory {
        lane: EvidenceLane::Event,
        owner: "sim-kernel Event and EventLedger",
        canonical_projection: "ordered typed event records",
    },
    EvidenceLaneInventory {
        lane: EvidenceLane::Receipt,
        owner: "the operation or library defining each typed receipt",
        canonical_projection: "ordered receipt identity and semantic fields",
    },
    EvidenceLaneInventory {
        lane: EvidenceLane::Browse,
        owner: "sim-kernel Card projection",
        canonical_projection: "ordered Card fields projected to Datum",
    },
    EvidenceLaneInventory {
        lane: EvidenceLane::Conformance,
        owner: "sim-lib-standard-core ConformanceHarness, ProfileDiff, and FidelityBadge",
        canonical_projection: "content-addressed test-run Datum and typed profile evidence",
    },
];

/// A formatting-only or ambient field that must not enter capture identity
/// without an explicit, scenario-declared projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExplicitProjectionField {
    /// Unstable field category.
    pub field: &'static str,
    /// Required treatment before capture identity is computed.
    pub required_projection: &'static str,
}

/// Known formatting-only and ambient inputs excluded from canonical identity.
pub const EXPLICIT_PROJECTION_FIELDS: &[ExplicitProjectionField] = &[
    ExplicitProjectionField {
        field: "Debug or Display rendering",
        required_projection: "project the typed value or record to canonical Datum",
    },
    ExplicitProjectionField {
        field: "wall-clock time and elapsed duration",
        required_projection: "omit, or replace with a named deterministic semantic field",
    },
    ExplicitProjectionField {
        field: "random seed and nondeterministic identifiers",
        required_projection: "declare the seed or map ids through a stable ordered projection",
    },
    ExplicitProjectionField {
        field: "host paths, process state, and environment",
        required_projection: "replace with scenario-declared logical identities",
    },
    ExplicitProjectionField {
        field: "unordered host collections",
        required_projection: "project with an explicit semantic ordering",
    },
];

/// Two existing source cases frozen as parity fixtures for capture development.
///
/// The pair deliberately covers a successful typed lowering and a declared,
/// coded gap. They preserve the pre-capture matrix behavior while later phases
/// add scenario execution and observation projection around it.
pub fn characterization_source_fixtures() -> [SourceConformanceCase; 2] {
    [
        SourceConformanceCase {
            symbol: Symbol::qualified("characterize", "lowering-parity"),
            organ: Symbol::qualified("standard", "lowering"),
            source_name: "lowering-parity.sim".to_owned(),
            source: "answer".to_owned(),
            kind: SourceConformanceCaseKind::Observed,
            expectation: SourceExpectation::LowersTo("answer".to_owned()),
            affects_badge: None,
        },
        SourceConformanceCase {
            symbol: Symbol::qualified("characterize", "declared-gap-parity"),
            organ: Symbol::qualified("standard", "unsupported"),
            source_name: "declared-gap-parity.sim".to_owned(),
            source: "ambient-clock".to_owned(),
            kind: SourceConformanceCaseKind::Observed,
            expectation: SourceExpectation::ExpectedGap {
                code: Symbol::qualified("characterize", "ambient-input"),
                reason: "ambient time is not a declared scenario input".to_owned(),
            },
            affects_badge: None,
        },
    ]
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, sync::Arc};

    use super::*;
    use crate::{MatrixRunner, SourceObservation};
    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};

    #[test]
    fn inventory_has_one_semantic_owner_and_projection_per_lane() {
        let mut lanes = BTreeSet::new();
        for entry in EVIDENCE_LANE_INVENTORY {
            assert!(
                lanes.insert(entry.lane as u8),
                "duplicate lane: {:?}",
                entry.lane
            );
            assert!(!entry.owner.is_empty());
            assert!(!entry.canonical_projection.is_empty());
            assert!(!entry.canonical_projection.contains("Debug"));
            assert!(!entry.canonical_projection.contains("debug"));
        }
        assert_eq!(lanes.len(), 7);
    }

    #[test]
    fn unstable_fields_all_require_explicit_projection() {
        assert!(
            EXPLICIT_PROJECTION_FIELDS
                .iter()
                .any(|field| field.field.contains("Debug"))
        );
        for field in EXPLICIT_PROJECTION_FIELDS {
            assert!(!field.required_projection.is_empty());
        }
    }

    #[test]
    fn frozen_source_fixtures_preserve_pass_and_gap_matrix_behavior() {
        let [pass, gap] = characterization_source_fixtures();
        let profile = crate::LanguageProfile::new(Symbol::qualified("characterize", "profile"));
        let row = crate::LanguageRowBuilder::new(Symbol::new("characterize"), profile)
            .with_cases([pass, gap])
            .build();
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let report = MatrixRunner::run_source_row(&mut cx, &row, |_cx, case| {
            Ok(match &case.expectation {
                SourceExpectation::LowersTo(value) => SourceObservation::LowersTo(value.clone()),
                SourceExpectation::ExpectedGap { code, reason } => SourceObservation::Gap {
                    code: code.clone(),
                    reason: reason.clone(),
                },
            })
        });

        assert_eq!(report.pass_count(), 1);
        assert_eq!(report.gap_count(), 1);
        assert_eq!(report.fail_count(), 0);
    }
}
