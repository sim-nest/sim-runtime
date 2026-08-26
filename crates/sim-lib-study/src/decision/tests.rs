//! Decision and report conformance tests.

use super::*;
use sha2::{Digest, Sha256};

fn id(name: &str) -> ContentId {
    ContentId::from_bytes(sym(name), Sha256::digest(name).into())
}
fn spend(comparable: u64) -> SpendSummary {
    summarize_spend(&[AttributedResourceEvent {
        evidence: id("spend"),
        subject: None,
        route: None,
        resource: sym("tokens"),
        amount: comparable,
        cause: ResourceCause::CandidateRoute,
    }])
}
fn row(name: &str, quality: Option<f64>, cost: Option<f64>, epochs: usize) -> SubjectEvidence {
    let mut values = BTreeMap::new();
    if let Some(value) = quality {
        values.insert(sym("quality"), value);
    }
    if let Some(value) = cost {
        values.insert(sym("cost"), value);
    }
    SubjectEvidence {
        subject: id(name),
        values,
        evidence: BTreeSet::from([id(&format!("evidence-{name}"))]),
        inferences: BTreeSet::from([id(&format!("inference-{name}"))]),
        attributions: BTreeSet::from([id(&format!("attribution-{name}"))]),
        epochs: (0..epochs)
            .map(|n| id(&format!("epoch-{name}-{n}")))
            .collect(),
        spend: spend(cost.unwrap_or(0.0) as u64),
        gate_rejected: false,
        unresolved: false,
        bootstrap: false,
        private_unapproved: false,
        report_only: false,
        quarantined: false,
    }
}
fn spec() -> DecisionSpec {
    DecisionSpec {
        id: id("spec"),
        evidence_root: id("root"),
        dimensions: vec![
            DimensionSpec {
                name: sym("quality"),
                direction: Direction::Maximize,
                equivalence: 0.01,
                required: true,
            },
            DimensionSpec {
                name: sym("cost"),
                direction: Direction::Minimize,
                equivalence: 0.01,
                required: true,
            },
        ],
        budget: Some(100),
        negative_epoch_floor: 2,
        expiry: id("expiry"),
    }
}

#[test]
fn f64_adapter_stamps_complete_call_and_refuses_other_domains() {
    let call = InferenceCall {
        operation: sym("mean-interval"),
        implementation: id("stats-v1"),
        inputs: vec![id("a"), id("b")],
        parameters: BTreeMap::new(),
    };
    let estimate = statistic_estimate(
        &sym("binary64"),
        1.0,
        0.5,
        1.5,
        0.95,
        &call,
        EvidenceClass::Publishable,
    )
    .unwrap();
    assert_eq!(estimate.inference, call.content_id().unwrap());
    assert_eq!(estimate.point.canonical, "1");
    assert_eq!(
        statistic_estimate(
            &sym("decimal"),
            1.0,
            0.0,
            2.0,
            0.95,
            &call,
            EvidenceClass::Publishable
        ),
        Err(DecisionError::UnsupportedNumberDomain)
    );
}

#[test]
fn aggregation_preserves_strata_counts_failures_and_provenance() {
    let samples = vec![
        FacetSample {
            evidence: id("a"),
            subject: id("s"),
            facet: sym("quality"),
            route: id("r1"),
            environment: id("e1"),
            value: Some(2.0),
            interval: Some((1.0, 3.0)),
            censored: false,
            failed: false,
        },
        FacetSample {
            evidence: id("b"),
            subject: id("s"),
            facet: sym("quality"),
            route: id("r2"),
            environment: id("e1"),
            value: None,
            interval: None,
            censored: true,
            failed: true,
        },
    ];
    let aggregate = aggregate_facets(&samples).unwrap().remove(0);
    assert_eq!(
        (
            aggregate.observed,
            aggregate.censored,
            aggregate.failures,
            aggregate.strata.len(),
            aggregate.provenance.len()
        ),
        (1, 1, 1, 2, 2)
    );
}

#[test]
fn only_declared_pairs_are_causal() {
    let left = id("left");
    let right = id("right");
    assert_eq!(
        contrast(left.clone(), right.clone(), 1.0, 2.0, None)
            .unwrap()
            .kind,
        ContrastKind::Descriptive
    );
    let paired = PairedTreatment {
        left_treatment: left.clone(),
        right_treatment: right.clone(),
        pair_keys: BTreeSet::from([id("pair")]),
    };
    assert_eq!(
        contrast(left, right, 1.0, 2.0, Some(&paired)).unwrap().kind,
        ContrastKind::Causal
    );
}

#[test]
fn spend_and_zero_accepted_economics_are_conservative() {
    let events = vec![
        AttributedResourceEvent {
            evidence: id("candidate"),
            subject: Some(id("s")),
            route: Some(id("r")),
            resource: sym("tokens"),
            amount: 10,
            cause: ResourceCause::CandidateRoute,
        },
        AttributedResourceEvent {
            evidence: id("observer"),
            subject: None,
            route: None,
            resource: sym("tokens"),
            amount: 4,
            cause: ResourceCause::Observer,
        },
    ];
    let summary = summarize_spend(&events);
    let economics = cost_per_accepted(&summary, 0);
    assert_eq!((summary.total, summary.comparable), (14, 10));
    assert_eq!(economics.cost_per_accepted, None);
    assert!(economics.verdict_censored);
}

#[test]
fn missing_cell_specialist_and_cheaper_equivalent_have_conservative_results() {
    let missing = decide(
        spec(),
        vec![
            row("a", Some(0.9), Some(5.0), 2),
            row("b", None, Some(4.0), 2),
        ],
    )
    .unwrap();
    assert_eq!(
        missing
            .subjects
            .iter()
            .find(|r| r.subject == id("b"))
            .unwrap()
            .verdict,
        Verdict::InsufficientEvidence
    );
    let specialist = decide(
        spec(),
        vec![
            row("fast", Some(0.8), Some(2.0), 2),
            row("specialist", Some(0.95), Some(80.0), 2),
        ],
    )
    .unwrap();
    assert!(
        specialist
            .subjects
            .iter()
            .any(|r| r.subject == id("specialist") && r.verdict == Verdict::Incomparable)
    );
    let cheaper = decide(
        spec(),
        vec![
            row("cheap", Some(0.9), Some(5.0), 2),
            row("dear", Some(0.9), Some(10.0), 2),
        ],
    )
    .unwrap();
    assert!(cheaper.edges.iter().any(|edge| edge.left == id("cheap")
        && edge.right == id("dear")
        && edge.relation == Relation::Dominates));
}

#[test]
fn epoch_floor_selection_admission_and_views_are_stable() {
    let specification = spec();
    assert!(!negative_projection_allowed(
        &specification,
        &row("a", Some(1.0), Some(1.0), 1).epochs
    ));
    let evidence = vec![
        row("a", Some(1.0), Some(1.0), 2),
        row("b", Some(0.5), Some(2.0), 2),
    ];
    let report = decide(specification, evidence.clone()).unwrap();
    let selection = select(
        &report,
        &evidence,
        StalenessInputs {
            subject_snapshot: id("snapshot"),
            evidence_root: id("root"),
            policy: id("spec"),
        },
    )
    .unwrap();
    assert_eq!(selection.subjects, vec![id("a")]);
    assert_eq!(report.render_sim(), report.render_sim());
    assert_eq!(report.render_markdown(), report.render_markdown());
    assert_eq!(report.render_csv(), report.render_csv());
    assert_eq!(report.render_table(), report.render_table());
    let mut inadmissible = evidence;
    inadmissible[0].bootstrap = true;
    assert_eq!(
        select(
            &report,
            &inadmissible,
            StalenessInputs {
                subject_snapshot: id("snapshot"),
                evidence_root: id("root"),
                policy: id("spec")
            }
        ),
        Err(DecisionError::InadmissibleSelection)
    );
}
