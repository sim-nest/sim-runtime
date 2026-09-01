//! Sequential study-design tests.

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
