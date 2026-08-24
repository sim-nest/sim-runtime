//! Sealed, replayable experimental design for staged studies.
//!
//! This module decides which already-sealed cells may be attempted. It does
//! not calculate statistics or execute work. Exploratory screening and
//! selection are permanently labelled as such; confirmation is a separately
//! sealed, fixed matrix.

use sha2::{Digest, Sha256};
use sim_kernel::{ContentId, Symbol};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

const TOTAL_SCREEN_ERROR_PPM: u32 = 20_000;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DesignCell {
    pub id: ContentId,
    pub subject: ContentId,
    pub task: ContentId,
    pub sample: u32,
    pub route: ContentId,
    pub host: ContentId,
    pub upper_exposure: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EliminationPolicy {
    pub id: ContentId,
    pub total_error_ppm: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolutionPolicy {
    pub id: ContentId,
    pub max_selected_cells: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmationDesign {
    pub id: ContentId,
    pub required_controls: BTreeSet<ContentId>,
}

/// All authority that can affect trial count or interpretation, fixed before
/// the first attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DesignSeal {
    pub id: ContentId,
    pub subject_snapshot: ContentId,
    pub pool: BTreeMap<ContentId, DesignCell>,
    pub registered_looks: Vec<u32>,
    pub block_seed: [u8; 32],
    pub controls: BTreeSet<ContentId>,
    pub elimination: EliminationPolicy,
    pub resolution: ResolutionPolicy,
    pub confirmation: ConfirmationDesign,
    pub max_retries: u32,
    pub concurrency: u32,
    pub exposure_ceiling: u64,
}

impl DesignSeal {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        subject_snapshot: ContentId,
        cells: Vec<DesignCell>,
        registered_looks: Vec<u32>,
        block_seed: [u8; 32],
        controls: BTreeSet<ContentId>,
        elimination: EliminationPolicy,
        resolution: ResolutionPolicy,
        confirmation: ConfirmationDesign,
        max_retries: u32,
        concurrency: u32,
        exposure_ceiling: u64,
    ) -> Result<Self, DesignError> {
        if cells.is_empty()
            || registered_looks.is_empty()
            || concurrency == 0
            || exposure_ceiling == 0
        {
            return Err(DesignError::InvalidSeal);
        }
        if elimination.total_error_ppm != TOTAL_SCREEN_ERROR_PPM
            || resolution.max_selected_cells == 0
        {
            return Err(DesignError::InvalidSeal);
        }
        let mut previous = 0;
        for look in &registered_looks {
            if *look <= previous {
                return Err(DesignError::InvalidSeal);
            }
            previous = *look;
        }
        let cell_count = cells.len();
        let pool = cells
            .into_iter()
            .map(|cell| (cell.id.clone(), cell))
            .collect::<BTreeMap<_, _>>();
        if pool.len() != cell_count
            || !controls.is_subset(&pool.keys().cloned().collect())
            || !confirmation.required_controls.is_subset(&controls)
        {
            return Err(DesignError::InvalidSeal);
        }
        let id = digest("design-seal-v1", |out| {
            put_id(out, &subject_snapshot);
            for cell in pool.values() {
                put_cell(out, cell);
            }
            for look in &registered_looks {
                put_u32(out, *look);
            }
            out.extend_from_slice(&block_seed);
            for control in &controls {
                put_id(out, control);
            }
            put_id(out, &elimination.id);
            put_u32(out, elimination.total_error_ppm);
            put_id(out, &resolution.id);
            put_u32(out, resolution.max_selected_cells);
            put_id(out, &confirmation.id);
            for control in &confirmation.required_controls {
                put_id(out, control);
            }
            put_u32(out, max_retries);
            put_u32(out, concurrency);
            put_u64(out, exposure_ceiling);
        });
        Ok(Self {
            id,
            subject_snapshot,
            pool,
            registered_looks,
            block_seed,
            controls,
            elimination,
            resolution,
            confirmation,
            max_retries,
            concurrency,
            exposure_ceiling,
        })
    }
}

/// Smoke output deliberately has no quality-bearing variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SmokeDiagnosis {
    Ready,
    RouteFailure { route: ContentId },
    ContractFailure { contract: ContentId },
}

pub fn smoke(
    seal: &DesignSeal,
    cell: &ContentId,
    route_ok: bool,
    contract_ok: bool,
) -> Result<SmokeDiagnosis, DesignError> {
    let cell = seal.pool.get(cell).ok_or(DesignError::OutsideSealedPool)?;
    Ok(if !route_ok {
        SmokeDiagnosis::RouteFailure {
            route: cell.route.clone(),
        }
    } else if !contract_ok {
        SmokeDiagnosis::ContractFailure {
            contract: cell.task.clone(),
        }
    } else {
        SmokeDiagnosis::Ready
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreeningDecision {
    Keep,
    Eliminate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvidenceClass {
    Exploratory,
    Confirmatory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreeningReceipt {
    pub id: ContentId,
    pub seal: ContentId,
    pub cell: ContentId,
    pub look: u32,
    pub seed: [u8; 32],
    pub error_spent_ppm: u32,
    pub decision: ScreeningDecision,
    pub evidence: EvidenceClass,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SequentialScreen {
    seal: ContentId,
    pool: BTreeSet<ContentId>,
    looks: Vec<u32>,
    seed: [u8; 32],
    error_spent_ppm: u32,
    receipts: Vec<ScreeningReceipt>,
}

impl SequentialScreen {
    pub fn new(seal: &DesignSeal) -> Self {
        Self {
            seal: seal.id.clone(),
            pool: seal.pool.keys().cloned().collect(),
            looks: seal.registered_looks.clone(),
            seed: seal.block_seed,
            error_spent_ppm: 0,
            receipts: Vec::new(),
        }
    }

    pub fn decide(
        &mut self,
        cell: &ContentId,
        look: u32,
        error_ppm: u32,
        decision: ScreeningDecision,
    ) -> Result<&ScreeningReceipt, DesignError> {
        if !self.pool.contains(cell) {
            return Err(DesignError::OutsideSealedPool);
        }
        if !self.looks.contains(&look) {
            return Err(DesignError::UnregisteredLook);
        }
        if self.error_spent_ppm.saturating_add(error_ppm) > TOTAL_SCREEN_ERROR_PPM {
            return Err(DesignError::ErrorBudget);
        }
        let seed = receipt_seed(&self.seed, cell, look);
        let id = digest("screen-receipt-v1", |out| {
            put_id(out, &self.seal);
            put_id(out, cell);
            put_u32(out, look);
            out.extend_from_slice(&seed);
            put_u32(out, error_ppm);
            out.push(decision as u8);
        });
        self.error_spent_ppm += error_ppm;
        self.receipts.push(ScreeningReceipt {
            id,
            seal: self.seal.clone(),
            cell: cell.clone(),
            look,
            seed,
            error_spent_ppm: error_ppm,
            decision,
            evidence: EvidenceClass::Exploratory,
        });
        Ok(self.receipts.last().expect("just appended"))
    }

    pub fn replay(&self, receipt: &ScreeningReceipt) -> bool {
        receipt.seal == self.seal
            && receipt.evidence == EvidenceClass::Exploratory
            && receipt.seed == receipt_seed(&self.seed, &receipt.cell, receipt.look)
            && self.pool.contains(&receipt.cell)
            && self.looks.contains(&receipt.look)
    }

    pub fn survivors(&self) -> BTreeSet<ContentId> {
        let eliminated = self
            .receipts
            .iter()
            .filter(|r| r.decision == ScreeningDecision::Eliminate)
            .map(|r| r.cell.clone())
            .collect::<BTreeSet<_>>();
        self.pool.difference(&eliminated).cloned().collect()
    }

    pub fn receipts(&self) -> &[ScreeningReceipt] {
        &self.receipts
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecisionChangeInput {
    pub cell: ContentId,
    pub expected_change: u64,
    pub exposure: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionReceipt {
    pub id: ContentId,
    pub seal: ContentId,
    pub inputs: Vec<DecisionChangeInput>,
    pub selected: Vec<ContentId>,
    pub evidence: EvidenceClass,
}

pub fn select_expected_decision_change(
    seal: &DesignSeal,
    survivors: &BTreeSet<ContentId>,
    mut inputs: Vec<DecisionChangeInput>,
) -> Result<SelectionReceipt, DesignError> {
    if inputs
        .iter()
        .any(|input| !seal.pool.contains_key(&input.cell) || !survivors.contains(&input.cell))
    {
        return Err(DesignError::OutsideSealedPool);
    }
    inputs.sort_by(|a, b| {
        b.expected_change
            .cmp(&a.expected_change)
            .then(a.exposure.cmp(&b.exposure))
            .then(a.cell.cmp(&b.cell))
    });
    let selected = inputs
        .iter()
        .take(seal.resolution.max_selected_cells as usize)
        .map(|input| input.cell.clone())
        .collect::<Vec<_>>();
    let id = digest("selection-receipt-v1", |out| {
        put_id(out, &seal.id);
        for input in &inputs {
            put_id(out, &input.cell);
            put_u64(out, input.expected_change);
            put_u64(out, input.exposure);
        }
        for cell in &selected {
            put_id(out, cell);
        }
    });
    Ok(SelectionReceipt {
        id,
        seal: seal.id.clone(),
        inputs,
        selected,
        evidence: EvidenceClass::Exploratory,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfirmationMatrix {
    pub id: ContentId,
    pub cells: Vec<ContentId>,
    pub evidence: EvidenceClass,
}

pub fn seal_confirmation(
    seal: &DesignSeal,
    survivors: &BTreeSet<ContentId>,
    proposed_negative: &BTreeSet<ContentId>,
) -> Result<ConfirmationMatrix, DesignError> {
    if survivors
        .iter()
        .chain(proposed_negative)
        .any(|cell| !seal.pool.contains_key(cell))
    {
        return Err(DesignError::OutsideSealedPool);
    }
    let cells = survivors
        .iter()
        .chain(proposed_negative)
        .chain(&seal.confirmation.required_controls)
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    if cells.is_empty() {
        return Err(DesignError::InvalidConfirmation);
    }
    let id = digest("confirmation-matrix-v1", |out| {
        put_id(out, &seal.id);
        for cell in &cells {
            put_id(out, cell);
        }
    });
    Ok(ConfirmationMatrix {
        id,
        cells,
        evidence: EvidenceClass::Confirmatory,
    })
}

/// Deterministic common-task/sample blocks. The caller-sealed concurrency is
/// merely enforced here; no route or endpoint policy is inferred.
pub fn schedule_blocks(
    seal: &DesignSeal,
    cells: &BTreeSet<ContentId>,
) -> Result<Vec<Vec<ContentId>>, DesignError> {
    if cells.iter().any(|id| !seal.pool.contains_key(id)) {
        return Err(DesignError::OutsideSealedPool);
    }
    let mut groups = BTreeMap::<(ContentId, u32), Vec<&DesignCell>>::new();
    for id in cells {
        let cell = &seal.pool[id];
        groups
            .entry((cell.task.clone(), cell.sample))
            .or_default()
            .push(cell);
    }
    let mut blocks = Vec::new();
    for ((task, sample), mut group) in groups {
        group.sort_by_key(|cell| balance_key(&seal.block_seed, &task, sample, cell));
        for chunk in group.chunks(seal.concurrency as usize) {
            blocks.push(chunk.iter().map(|cell| cell.id.clone()).collect());
        }
    }
    Ok(blocks)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetDecision {
    Authorized { remaining_after: u64 },
    Incomplete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExposureBudget {
    ceiling: u64,
    spent: u64,
}

impl ExposureBudget {
    pub fn new(seal: &DesignSeal) -> Self {
        Self {
            ceiling: seal.exposure_ceiling,
            spent: 0,
        }
    }
    /// Recomputes conservative remaining exposure immediately before a metered action.
    pub fn authorize(&mut self, conservative_upper: u64) -> BudgetDecision {
        let remaining = self.ceiling.saturating_sub(self.spent);
        if conservative_upper > remaining {
            return BudgetDecision::Incomplete;
        }
        self.spent += conservative_upper;
        BudgetDecision::Authorized {
            remaining_after: self.ceiling - self.spent,
        }
    }
    pub fn spent(&self) -> u64 {
        self.spent
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpochInputs {
    pub subject: ContentId,
    pub task: ContentId,
    pub price: ContentId,
    pub environment: ContentId,
    pub drift_policy: ContentId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StudyEpoch {
    pub id: ContentId,
    pub ordinal: u32,
    pub inputs: EpochInputs,
}

impl StudyEpoch {
    pub fn open(ordinal: u32, inputs: EpochInputs) -> Self {
        let id = digest("study-epoch-v1", |out| {
            put_u32(out, ordinal);
            put_epoch(out, &inputs);
        });
        Self {
            id,
            ordinal,
            inputs,
        }
    }
    pub fn renew_if_changed(&self, current: EpochInputs) -> Option<Self> {
        (self.inputs != current).then(|| Self::open(self.ordinal + 1, current))
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DesignError {
    #[error("invalid or incomplete design seal")]
    InvalidSeal,
    #[error("cell is outside the sealed pool")]
    OutsideSealedPool,
    #[error("screening look was not registered")]
    UnregisteredLook,
    #[error("screening total error budget would exceed .02")]
    ErrorBudget,
    #[error("confirmation matrix is empty")]
    InvalidConfirmation,
}

fn receipt_seed(seed: &[u8; 32], cell: &ContentId, look: u32) -> [u8; 32] {
    let mut bytes = seed.to_vec();
    put_id(&mut bytes, cell);
    put_u32(&mut bytes, look);
    Sha256::digest(bytes).into()
}
fn balance_key(seed: &[u8; 32], task: &ContentId, sample: u32, cell: &DesignCell) -> [u8; 32] {
    let mut bytes = seed.to_vec();
    put_id(&mut bytes, task);
    put_u32(&mut bytes, sample);
    put_id(&mut bytes, &cell.subject);
    put_id(&mut bytes, &cell.route);
    put_id(&mut bytes, &cell.host);
    Sha256::digest(bytes).into()
}
fn digest(name: &str, encode: impl FnOnce(&mut Vec<u8>)) -> ContentId {
    let mut bytes = Vec::new();
    encode(&mut bytes);
    ContentId::from_bytes(
        Symbol::qualified("study", name),
        Sha256::digest(bytes).into(),
    )
}
fn put_id(out: &mut Vec<u8>, id: &ContentId) {
    let qualified = id.algorithm.as_qualified_str();
    let name = qualified.as_bytes();
    put_u32(out, name.len() as u32);
    out.extend_from_slice(name);
    out.extend_from_slice(&id.bytes);
}
fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}
fn put_cell(out: &mut Vec<u8>, cell: &DesignCell) {
    put_id(out, &cell.id);
    put_id(out, &cell.subject);
    put_id(out, &cell.task);
    put_u32(out, cell.sample);
    put_id(out, &cell.route);
    put_id(out, &cell.host);
    put_u64(out, cell.upper_exposure);
}
fn put_epoch(out: &mut Vec<u8>, value: &EpochInputs) {
    put_id(out, &value.subject);
    put_id(out, &value.task);
    put_id(out, &value.price);
    put_id(out, &value.environment);
    put_id(out, &value.drift_policy);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(name: &str) -> ContentId {
        ContentId::from_bytes(Symbol::qualified("test", name), Sha256::digest(name).into())
    }
    fn fixture() -> DesignSeal {
        let cells = (0..8)
            .map(|n| DesignCell {
                id: id(&format!("cell-{n}")),
                subject: id(&format!("subject-{}", n % 2)),
                task: id("task"),
                sample: (n / 4) as u32,
                route: id(&format!("route-{}", n % 2)),
                host: id(&format!("host-{}", n % 2)),
                upper_exposure: 10,
            })
            .collect::<Vec<_>>();
        let controls = BTreeSet::from([cells[0].id.clone()]);
        DesignSeal::new(
            id("snapshot"),
            cells,
            vec![2, 4],
            [7; 32],
            controls.clone(),
            EliminationPolicy {
                id: id("elimination"),
                total_error_ppm: TOTAL_SCREEN_ERROR_PPM,
            },
            ResolutionPolicy {
                id: id("resolution"),
                max_selected_cells: 3,
            },
            ConfirmationDesign {
                id: id("confirmation"),
                required_controls: controls,
            },
            2,
            2,
            35,
        )
        .unwrap()
    }

    #[test]
    fn staged_design_saves_cells_and_fixed_confirmation_recovers_frontier() {
        let seal = fixture();
        let mut screen = SequentialScreen::new(&seal);
        let ids = seal.pool.keys().cloned().collect::<Vec<_>>();
        for cell in &ids[4..] {
            screen
                .decide(cell, 2, 4_000, ScreeningDecision::Eliminate)
                .unwrap();
        }
        let survivors = screen.survivors();
        let selection = select_expected_decision_change(
            &seal,
            &survivors,
            survivors
                .iter()
                .enumerate()
                .map(|(n, cell)| DecisionChangeInput {
                    cell: cell.clone(),
                    expected_change: 100 - n as u64,
                    exposure: 10,
                })
                .collect(),
        )
        .unwrap();
        assert!(selection.selected.len() < seal.pool.len());
        assert_eq!(selection.evidence, EvidenceClass::Exploratory);
        let known_negative = BTreeSet::from([ids[7].clone()]);
        let confirmation = seal_confirmation(
            &seal,
            &selection.selected.iter().cloned().collect(),
            &known_negative,
        )
        .unwrap();
        assert!(confirmation.cells.contains(&ids[7]));
        assert!(
            seal.confirmation
                .required_controls
                .is_subset(&confirmation.cells.iter().cloned().collect())
        );
        assert_eq!(confirmation.evidence, EvidenceClass::Confirmatory);
    }

    #[test]
    fn elimination_replays_and_smoke_or_failures_never_become_quality_evidence() {
        let seal = fixture();
        let cell = seal.pool.keys().next().unwrap().clone();
        let mut screen = SequentialScreen::new(&seal);
        let receipt = screen
            .decide(&cell, 2, 20_000, ScreeningDecision::Eliminate)
            .unwrap()
            .clone();
        assert!(screen.replay(&receipt));
        assert_eq!(
            screen.decide(&cell, 4, 1, ScreeningDecision::Keep),
            Err(DesignError::ErrorBudget)
        );
        assert!(matches!(
            smoke(&seal, &cell, false, true).unwrap(),
            SmokeDiagnosis::RouteFailure { .. }
        ));
        assert!(matches!(
            smoke(&seal, &cell, true, false).unwrap(),
            SmokeDiagnosis::ContractFailure { .. }
        ));
    }

    #[test]
    fn blocks_are_paired_budget_stops_incomplete_and_epoch_changes_are_immutable() {
        let seal = fixture();
        let all = seal.pool.keys().cloned().collect();
        let blocks = schedule_blocks(&seal, &all).unwrap();
        assert!(
            blocks
                .iter()
                .all(|block| block.len() <= seal.concurrency as usize)
        );
        assert!(blocks.iter().all(|block| {
            let first = &seal.pool[&block[0]];
            block
                .iter()
                .all(|id| seal.pool[id].task == first.task && seal.pool[id].sample == first.sample)
        }));
        let mut budget = ExposureBudget::new(&seal);
        assert_eq!(
            budget.authorize(30),
            BudgetDecision::Authorized { remaining_after: 5 }
        );
        assert_eq!(budget.authorize(10), BudgetDecision::Incomplete);
        assert_eq!(budget.spent(), 30);
        let inputs = EpochInputs {
            subject: id("s"),
            task: id("t"),
            price: id("p"),
            environment: id("e"),
            drift_policy: id("d"),
        };
        let epoch = StudyEpoch::open(0, inputs.clone());
        assert!(epoch.renew_if_changed(inputs.clone()).is_none());
        let next = epoch
            .renew_if_changed(EpochInputs {
                price: id("new-price"),
                ..inputs
            })
            .unwrap();
        assert_eq!(next.ordinal, 1);
        assert_ne!(next.id, epoch.id);
    }
}
