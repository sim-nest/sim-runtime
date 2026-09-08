use std::collections::{BTreeMap, BTreeSet};

use sim_kernel::Datum;

use super::*;

const DENOMINATOR: usize = 40;

#[derive(Clone, Copy)]
enum Mutation {
    Declaration(usize),
    Edge(usize, usize),
}

fn fact(value: &str) -> FactId {
    FactId::new(value).unwrap()
}

fn conclusion(index: usize) -> ConclusionId {
    ConclusionId::new(format!("conclusion/{index:02}")).unwrap()
}

fn scenario_fact(index: usize) -> Option<FactId> {
    match index {
        1..=2 => Some(fact("change/private-implementation")),
        3..=10 => Some(fact("change/public-api")),
        11..=13 => Some(fact("change/head-a")),
        14..=16 => Some(fact("change/head-b")),
        17..=19 => Some(fact("change/head-c")),
        20..=22 => Some(fact("change/control-pins")),
        23..=24 => Some(fact("change/proof-code-alpha")),
        25..=26 => Some(fact("change/checker-alpha")),
        _ => None,
    }
}

fn changed_fact_candidates() -> Vec<FactId> {
    [
        "change/private-implementation",
        "change/public-api",
        "change/head-a",
        "change/head-b",
        "change/head-c",
        "change/control-pins",
        "change/proof-code-alpha",
        "change/checker-alpha",
    ]
    .map(fact)
    .to_vec()
}

fn closure(mutation: Option<Mutation>) -> FederatedClosure {
    let mut universe = changed_fact_candidates()
        .into_iter()
        .collect::<BTreeSet<_>>();
    universe.insert(fact("change/dormant"));
    let mut by_owner = BTreeMap::<String, Vec<(ConclusionId, BTreeSet<FactId>)>>::new();
    for index in 1..=DENOMINATOR {
        let base = fact(&format!("stable/{index:02}"));
        universe.insert(base.clone());
        let mut dependencies = BTreeSet::from([base]);
        if let Some(changed) = scenario_fact(index) {
            dependencies.insert(changed);
        }
        match mutation {
            Some(Mutation::Declaration(target)) if target == index => {
                if scenario_fact(index).is_some() {
                    continue;
                }
                dependencies.insert(fact("change/private-implementation"));
            }
            Some(Mutation::Edge(target, edge)) if target == index => {
                let original = dependencies.iter().nth(edge).unwrap().clone();
                dependencies.remove(&original);
                if original.as_str().starts_with("change/") {
                    dependencies.insert(fact("change/dormant"));
                } else {
                    let replacement = changed_fact_candidates()
                        .into_iter()
                        .find(|candidate| !dependencies.contains(candidate))
                        .unwrap();
                    dependencies.insert(replacement);
                }
            }
            _ => {}
        }
        by_owner
            .entry(format!("owner/{:02}", (index - 1) / 10 + 1))
            .or_default()
            .push((conclusion(index), dependencies));
    }
    let graphs = by_owner
        .into_iter()
        .map(|(owner, dependencies)| OwnerProjectionGraph::new(owner, dependencies).unwrap());
    FederatedClosure::seal(universe, graphs).unwrap()
}

fn affected(range: std::ops::RangeInclusive<usize>) -> BTreeSet<ConclusionId> {
    range.map(conclusion).collect()
}

fn expected_rows() -> Vec<ExpectedClosure> {
    vec![
        ExpectedClosure {
            delta: "D1".into(),
            denominator: DENOMINATOR,
            max_affected: 0,
            max_revision_delta: 0,
            carried_state: false,
            affected: BTreeSet::new(),
        },
        ExpectedClosure {
            delta: "D2".into(),
            denominator: DENOMINATOR,
            max_affected: 2,
            max_revision_delta: 1,
            carried_state: false,
            affected: affected(1..=2),
        },
        ExpectedClosure {
            delta: "D3".into(),
            denominator: DENOMINATOR,
            max_affected: 8,
            max_revision_delta: 1,
            carried_state: false,
            affected: affected(3..=10),
        },
        ExpectedClosure {
            delta: "D4".into(),
            denominator: DENOMINATOR,
            max_affected: 12,
            max_revision_delta: 4,
            carried_state: false,
            affected: affected(11..=22),
        },
        ExpectedClosure {
            delta: "D5".into(),
            denominator: DENOMINATOR,
            max_affected: 4,
            max_revision_delta: 2,
            carried_state: false,
            affected: affected(23..=26),
        },
        ExpectedClosure {
            delta: "D6".into(),
            denominator: DENOMINATOR,
            max_affected: 0,
            max_revision_delta: 1,
            carried_state: false,
            affected: BTreeSet::new(),
        },
    ]
}

fn expected() -> ExpectedClosureSet {
    let source = Datum::Bytes(vec![7]).content_id().unwrap();
    ExpectedClosureSet::freeze(source, expected_rows()).unwrap()
}

fn deltas() -> Vec<ControlledDelta> {
    vec![
        ControlledDelta::new("D1", ControlledDeltaClass::NoSemanticChange, [], 0, false).unwrap(),
        ControlledDelta::new(
            "D2",
            ControlledDeltaClass::PrivateImplementation,
            [fact("change/private-implementation")],
            1,
            false,
        )
        .unwrap(),
        ControlledDelta::new(
            "D3",
            ControlledDeltaClass::PublicApi,
            [fact("change/public-api")],
            1,
            false,
        )
        .unwrap(),
        ControlledDelta::new(
            "D4",
            ControlledDeltaClass::PublicHeadsAndPins,
            [
                fact("change/head-a"),
                fact("change/head-b"),
                fact("change/head-c"),
                fact("change/control-pins"),
            ],
            4,
            false,
        )
        .unwrap(),
        ControlledDelta::new(
            "D5",
            ControlledDeltaClass::ProofAndCheckerImplementation,
            [
                fact("change/proof-code-alpha"),
                fact("change/checker-alpha"),
            ],
            2,
            false,
        )
        .unwrap(),
        ControlledDelta::new("D6", ControlledDeltaClass::PresentationOnly, [], 1, false).unwrap(),
    ]
}

#[test]
fn six_predictions_match_the_frozen_oracle_and_state_denominators() {
    let closure = closure(None);
    let qualification = PredictedClosureAssay::new(&closure)
        .qualify(&deltas(), &expected())
        .unwrap();
    assert_eq!(qualification.reports.len(), 6);
    let d1 = &qualification.reports[0];
    let d2 = &qualification.reports[1];
    let d3 = &qualification.reports[2];
    let d4 = &qualification.reports[3];
    let d5 = &qualification.reports[4];
    let d6 = &qualification.reports[5];
    assert_eq!((d1.denominator, d1.unaffected_basis_points), (40, 10_000));
    assert_eq!((d2.denominator, d2.unaffected_basis_points), (40, 9_500));
    assert_eq!(d3.affected, affected(3..=10));
    assert_eq!((d4.denominator, d4.unaffected_basis_points), (40, 7_000));
    assert_eq!(d5.affected, affected(23..=26));
    assert_eq!(d1.work.planned_starts, 0);
    assert_eq!(d1.work.journal_records, 0);
    assert_eq!(d6.work.planned_starts, 0);
    assert_eq!(d6.work.journal_records, 0);
    assert_eq!((d1.revision_delta, d1.carried_state), (0, false));
    assert_eq!((d6.revision_delta, d6.carried_state), (1, false));
    assert!(
        qualification
            .reports
            .iter()
            .all(|report| report.denominator == 40)
    );
}

#[test]
fn every_projection_declaration_and_graph_edge_has_a_killing_mutant() {
    let expected = expected();
    let deltas = deltas();
    for declaration in 1..=DENOMINATOR {
        let mutant = closure(Some(Mutation::Declaration(declaration)));
        assert!(
            matches!(
                PredictedClosureAssay::new(&mutant).qualify(&deltas, &expected),
                Err(AssayError::RepairRequired(_))
            ),
            "declaration mutant {declaration} survived"
        );
    }
    for declaration in 1..=DENOMINATOR {
        let edge_count = usize::from(scenario_fact(declaration).is_some()) + 1;
        for edge in 0..edge_count {
            let mutant = closure(Some(Mutation::Edge(declaration, edge)));
            assert!(
                matches!(
                    PredictedClosureAssay::new(&mutant).qualify(&deltas, &expected),
                    Err(AssayError::RepairRequired(_))
                ),
                "edge mutant {declaration}:{edge} survived"
            );
        }
    }
}

#[test]
fn misses_emit_bounded_repairs_and_third_unchanged_epoch_needs_direction() {
    let mutant = closure(Some(Mutation::Declaration(1)));
    let AssayOutcome::Repair { repairs, .. } = PredictedClosureAssay::new(&mutant)
        .predict(&deltas()[1], &expected())
        .unwrap()
    else {
        panic!("mutant unexpectedly passed")
    };
    assert!(
        repairs
            .items
            .contains(&ProjectionRepairItem::Declaration(conclusion(1)))
    );
    let mut tracker = ProjectionRepairTracker::default();
    assert_eq!(tracker.record(&repairs), RepairDisposition::RepairEpoch(1));
    assert_eq!(tracker.record(&repairs), RepairDisposition::RepairEpoch(2));
    assert!(matches!(
        tracker.record(&repairs),
        RepairDisposition::NeedDirection(ArchitectureFaultReview { epochs: 3, .. })
    ));
    assert!(matches!(
        tracker.record(&repairs),
        RepairDisposition::NeedDirection(ArchitectureFaultReview { epochs: 3, .. })
    ));
}

#[test]
fn semantic_no_ops_and_incomplete_suites_fail_closed() {
    assert!(matches!(
        ControlledDelta::new(
            "D6",
            ControlledDeltaClass::PresentationOnly,
            [fact("change/public-api")],
            1,
            false
        ),
        Err(AssayError::SemanticNoOpCarriesChangedFacts(_))
    ));
    let closure = closure(None);
    assert!(matches!(
        PredictedClosureAssay::new(&closure).qualify(&deltas()[..5], &expected()),
        Err(AssayError::IncompleteDeltaSet(_))
    ));

    let mut wrong_class = deltas();
    wrong_class[0].class = ControlledDeltaClass::PrivateImplementation;
    assert!(matches!(
        PredictedClosureAssay::new(&closure).qualify(&wrong_class, &expected()),
        Err(AssayError::RepairRequired(_))
    ));

    let mut unknown_fact = deltas();
    unknown_fact[1]
        .changed_facts
        .insert(fact("change/unsealed"));
    let AssayOutcome::Repair { repairs, .. } = PredictedClosureAssay::new(&closure)
        .predict(&unknown_fact[1], &expected())
        .unwrap()
    else {
        panic!("unsealed changed fact unexpectedly passed")
    };
    assert!(repairs.items.contains(&ProjectionRepairItem::Contract(
        AssayContract::UnknownChangedFact
    )));

    let mut invalid_rows = expected_rows();
    invalid_rows[1].max_affected = 3;
    invalid_rows[1].affected.insert(conclusion(3));
    let source = Datum::Bytes(vec![8]).content_id().unwrap();
    assert!(matches!(
        ExpectedClosureSet::freeze(source, invalid_rows),
        Err(AssayError::InvalidExpectedClosure { .. })
    ));
}
