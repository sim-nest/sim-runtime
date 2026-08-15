use std::collections::BTreeSet;

use super::*;

#[test]
fn every_excluded_candidate_has_an_exact_machine_reason() {
    let excluded = candidate_inventory()
        .iter()
        .filter(|row| row.disposition == CandidateDisposition::Exclude)
        .map(|row| row.model)
        .collect::<BTreeSet<_>>();
    let reasons = exclusion_ledger()
        .iter()
        .map(|row| {
            assert_eq!(row.required, ParentMeaning::DeclaredSuperclass);
            assert_ne!(row.actual, row.required);
            assert!(!row.mismatch_code.is_empty());
            row.model
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(excluded, reasons);
}

#[test]
fn every_exclusion_has_a_precise_non_goal() {
    let exclusions = exclusion_ledger()
        .iter()
        .map(|row| row.model)
        .collect::<BTreeSet<_>>();
    let goals = non_goals()
        .iter()
        .map(|row| {
            assert!(!row.excluded_semantics.is_empty());
            row.model
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(exclusions, goals);
}

#[test]
fn characterize_1_covers_success_and_every_failure_mode() {
    let scenarios = characterization_scenarios();
    for required in [
        ScenarioKind::C3Resolution,
        ScenarioKind::DeclaredParentTraversal,
        ScenarioKind::SubclassTest,
        ScenarioKind::ReadConstruction,
    ] {
        assert!(scenarios.iter().any(|scenario| scenario.kind == required));
    }
    for failure in [
        FailureMode::UnknownParent,
        FailureMode::DuplicateParent,
        FailureMode::InconsistentC3,
        FailureMode::ParentCycle,
        FailureMode::ReadConstructorMissing,
        FailureMode::ReadShapeMismatch,
    ] {
        assert!(
            scenarios
                .iter()
                .any(|scenario| scenario.kind == ScenarioKind::Failure(failure))
        );
    }
}

#[test]
fn characterize_1_replays_to_identical_content_ids() {
    let first = characterization_scenarios()
        .iter()
        .map(scenario_content_id)
        .collect::<Vec<_>>();
    let replay = characterization_scenarios()
        .iter()
        .map(scenario_content_id)
        .collect::<Vec<_>>();
    assert_eq!(first, replay);
    assert_eq!(first.iter().collect::<BTreeSet<_>>().len(), first.len());
}
