use std::collections::BTreeSet;

use sim_kernel::{ContentId, Datum, Symbol};

use super::{
    ConclusionId, FactId,
    assay::{AssayContract, AssayError},
};

/// One graph location requiring narrower or more complete projection sensitivity.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum ProjectionRepairItem {
    /// A conclusion declaration is absent or has the wrong sensitivity.
    Declaration(ConclusionId),
    /// A declared conclusion-to-fact edge is too broad.
    GraphEdge {
        /// Affected conclusion.
        conclusion: ConclusionId,
        /// Changed fact responsible for the false prediction.
        fact: FactId,
    },
    /// A non-graph assay contract was violated.
    Contract(AssayContract),
}

/// Canonical bounded set of projection declarations or graph edges to repair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionRepairSet {
    /// Controlled delta that exposed the finding.
    pub delta: String,
    /// Canonically ordered, deduplicated repair items.
    pub items: BTreeSet<ProjectionRepairItem>,
    id: ContentId,
}

impl ProjectionRepairSet {
    pub(super) fn new(
        delta: &str,
        items: BTreeSet<ProjectionRepairItem>,
    ) -> Result<Self, AssayError> {
        let datum = Datum::Node {
            tag: Symbol::qualified("projection", "repair-set-v1"),
            fields: vec![
                (Symbol::new("delta"), Datum::String(delta.to_owned())),
                (
                    Symbol::new("items"),
                    Datum::Vector(items.iter().map(repair_item_datum).collect()),
                ),
            ],
        };
        let id = datum
            .content_id()
            .map_err(|error| AssayError::Canonical(error.to_string()))?;
        Ok(Self {
            delta: delta.to_owned(),
            items,
            id,
        })
    }

    /// Returns the stable identity used to recognize an unchanged finding.
    #[must_use]
    pub fn id(&self) -> &ContentId {
        &self.id
    }
}

/// Architecture review emitted after three unchanged failed repair epochs.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArchitectureFaultReview {
    /// Unchanged finding identity.
    pub finding: ContentId,
    /// Exact number of consecutive epochs.
    pub epochs: u8,
}

/// Repair loop disposition. NeedDirection is the terminal third epoch.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepairDisposition {
    /// One bounded repair packet remains allowed.
    RepairEpoch(u8),
    /// Architecture review is required before more work or a higher rung.
    NeedDirection(ArchitectureFaultReview),
}

/// Tracks consecutive unchanged projection findings without silently waiving one.
#[derive(Clone, Debug, Default)]
pub struct ProjectionRepairTracker {
    finding: Option<ContentId>,
    epochs: u8,
}

impl ProjectionRepairTracker {
    /// Records a failed repair epoch, resetting the count when the finding changes.
    #[must_use]
    pub fn record(&mut self, repairs: &ProjectionRepairSet) -> RepairDisposition {
        if self.finding.as_ref() == Some(repairs.id()) {
            self.epochs = self.epochs.saturating_add(1).min(3);
        } else {
            self.finding = Some(repairs.id().clone());
            self.epochs = 1;
        }
        if self.epochs >= 3 {
            RepairDisposition::NeedDirection(ArchitectureFaultReview {
                finding: repairs.id().clone(),
                epochs: self.epochs,
            })
        } else {
            RepairDisposition::RepairEpoch(self.epochs)
        }
    }

    /// Clears consecutive failure state after a passing delta.
    pub fn clear(&mut self) {
        self.finding = None;
        self.epochs = 0;
    }
}

fn repair_item_datum(item: &ProjectionRepairItem) -> Datum {
    match item {
        ProjectionRepairItem::Declaration(conclusion) => Datum::Node {
            tag: Symbol::qualified("projection", "repair-declaration-v1"),
            fields: vec![(
                Symbol::new("conclusion"),
                Datum::String(conclusion.as_str().to_owned()),
            )],
        },
        ProjectionRepairItem::GraphEdge { conclusion, fact } => Datum::Node {
            tag: Symbol::qualified("projection", "repair-edge-v1"),
            fields: vec![
                (
                    Symbol::new("conclusion"),
                    Datum::String(conclusion.as_str().to_owned()),
                ),
                (Symbol::new("fact"), Datum::String(fact.as_str().to_owned())),
            ],
        },
        ProjectionRepairItem::Contract(contract) => Datum::Node {
            tag: Symbol::qualified("projection", "repair-contract-v1"),
            fields: vec![(
                Symbol::new("name"),
                Datum::String(assay_contract_name(*contract).into()),
            )],
        },
    }
}

const fn assay_contract_name(contract: AssayContract) -> &'static str {
    match contract {
        AssayContract::DeltaClass => "delta-class",
        AssayContract::UnknownChangedFact => "unknown-changed-fact",
        AssayContract::Denominator => "denominator",
        AssayContract::AffectedCeiling => "affected-ceiling",
        AssayContract::RevisionDelta => "revision-delta",
        AssayContract::CarriedState => "carried-state",
        AssayContract::D1UnaffectedFloor => "d1-unaffected-floor",
        AssayContract::D2UnaffectedFloor => "d2-unaffected-floor",
        AssayContract::D4UnaffectedFloor => "d4-unaffected-floor",
        AssayContract::D6CarriedState => "d6-carried-state",
        AssayContract::SemanticNoOpWork => "semantic-no-op-work",
    }
}
