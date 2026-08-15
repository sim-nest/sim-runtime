// conformance: characterization remains neutral across control and mutation organs.

use std::sync::{Arc, Mutex};

use sim_kernel::{Cx, Datum, DefaultFactory, NoopEvalPolicy, Symbol};
use sim_lib_control::{CleanupStack, Unwind};
use sim_lib_mutation::{
    EdgeId, EdgeVisitor, HardCappedRetainPolicy, ManagedArena, ManagedId, ManagedObject,
};

use crate::{
    BoundedLane, CanonicalObservation, CanonicalOutcome, CaptureComparisonProjection,
    CharacterizationCapture, CharacterizationScenario, ConformanceHarness, ScenarioInput,
    ScenarioLimits, ScenarioObservationLane, ScenarioSpec, compare_characterization_captures,
    publish_characterization_capture,
};

const PROJECTION: &str = "neutral-organs/v1";

#[test]
fn control_cleanup_capture_replays_exactly_without_a_guest_profile() {
    let (scenario, first) = cleanup_specimen();
    let (_, replay) = cleanup_specimen();

    let comparison = compare_characterization_captures(
        &scenario,
        &first,
        &scenario,
        &replay,
        &strict_projection(),
    )
    .unwrap();

    assert!(comparison.is_same());
    assert_eq!(
        publish_characterization_capture(&mut test_cx(), &scenario, &first).unwrap(),
        publish_characterization_capture(&mut test_cx(), &scenario, &replay).unwrap()
    );
}

#[test]
fn mutation_collection_capture_locates_changed_edge_and_receipt() {
    let scenario =
        neutral_spec("mutation-collection", 2, 8).observing(ScenarioObservationLane::Receipts);
    let baseline = mutation_capture(false);
    let changed = mutation_capture(true);

    let comparison = compare_characterization_captures(
        &scenario,
        &baseline,
        &scenario,
        &changed,
        &strict_projection(),
    )
    .unwrap();

    assert!(!comparison.is_same());
    assert_eq!(
        comparison
            .differences
            .iter()
            .map(|difference| difference.path.as_str())
            .collect::<Vec<_>>(),
        [
            "$.observation.outcome.value.target",
            "$.observation.receipts.items[0].edge-target"
        ]
    );
}

#[test]
fn neutral_scenarios_fail_closed_on_bounds_authority_and_order() {
    let effects = Arc::new(Mutex::new(0usize));
    let driver_effects = effects.clone();
    let undeclared = ScenarioSpec::new(
        Symbol::qualified("scenario", "neutral-authority"),
        Symbol::qualified("setup", "control-organ/v1"),
    )
    .with_limits(ScenarioLimits::new(1, 1))
    .with_input(ScenarioInput::new(
        Symbol::new("unwind"),
        Symbol::qualified("authority", "control"),
        Datum::String("close".to_owned()),
    ))
    .observing(ScenarioObservationLane::ValueOrFailure);
    let mut harness = ConformanceHarness::new();
    harness
        .register_scenario(CharacterizationScenario::new(
            undeclared,
            Arc::new(move |_, _| {
                *driver_effects.lock().unwrap() += 1;
                Ok(())
            }),
        ))
        .unwrap();
    assert!(
        harness
            .run_scenarios(&mut test_cx())
            .unwrap_err()
            .to_string()
            .contains("undeclared authority")
    );
    assert_eq!(*effects.lock().unwrap(), 0, "refusal precedes effects");

    let bounded = neutral_spec("bounded", 1, 1).observing(ScenarioObservationLane::Receipts);
    let mut truncated =
        CharacterizationCapture::new(projection_symbol(), success(Datum::Bool(true)));
    truncated.observation.receipts = BoundedLane::capture(
        vec![
            Datum::String("first".into()),
            Datum::String("second".into()),
        ],
        1,
    );
    assert!(
        publish_characterization_capture(&mut test_cx(), &bounded, &truncated)
            .unwrap_err()
            .to_string()
            .contains("truncated Receipts")
    );

    let ordered = mutation_capture(false);
    let mut reordered = ordered.clone();
    let BoundedLane::Complete(receipts) = &mut reordered.observation.receipts else {
        panic!("mutation specimen records complete receipts")
    };
    receipts.reverse();
    let comparison = compare_characterization_captures(
        &neutral_spec("mutation-collection", 2, 8).observing(ScenarioObservationLane::Receipts),
        &ordered,
        &neutral_spec("mutation-collection", 2, 8).observing(ScenarioObservationLane::Receipts),
        &reordered,
        &strict_projection(),
    )
    .unwrap();
    assert_eq!(
        comparison
            .differences
            .iter()
            .map(|difference| difference.path.as_str())
            .collect::<Vec<_>>(),
        [
            "$.observation.receipts.items[0]",
            "$.observation.receipts.items[1]"
        ]
    );
}

fn cleanup_specimen() -> (ScenarioSpec, CharacterizationCapture) {
    type Reason = Unwind<&'static str, (), (), ()>;
    let scenario = neutral_spec("control-cleanup", 1, 4).observing(ScenarioObservationLane::Events);
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut stack = CleanupStack::new();
    for name in ["outer", "inner"] {
        let events = events.clone();
        stack.push(move |_| events.lock().unwrap().push(Datum::String(name.into())));
    }
    let reason = stack.unwind(Reason::Return("complete"));
    let outcome = match reason {
        Reason::Return(value) => Datum::String(value.into()),
        _ => unreachable!("the control specimen returns"),
    };
    let mut observation = success(outcome);
    observation.events = BoundedLane::Complete(events.lock().unwrap().clone());
    (
        scenario,
        CharacterizationCapture::new(projection_symbol(), observation),
    )
}

#[derive(Default)]
struct Node(Vec<ManagedId>);

impl ManagedObject for Node {
    fn trace_edges(&self, visitor: &mut dyn EdgeVisitor) {
        for (index, target) in self.0.iter().copied().enumerate() {
            visitor.strong(EdgeId(index as u32), target);
        }
    }

    fn clear_weak_edge(&mut self, _edge: EdgeId, _expected: ManagedId) -> bool {
        false
    }
}

fn mutation_capture(changed_edge: bool) -> CharacterizationCapture {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(3).unwrap());
    let root = arena.allocate(Node::default()).unwrap();
    let first = arena.allocate(Node::default()).unwrap();
    let second = arena.allocate(Node::default()).unwrap();
    arena.get_mut(root).unwrap().0.push(if changed_edge {
        second.id()
    } else {
        first.id()
    });
    arena.root(root).unwrap();
    let (target, safepoint) = arena
        .safepoint(|snapshot| {
            struct FirstStrong(Option<ManagedId>);
            impl EdgeVisitor for FirstStrong {
                fn strong(&mut self, _edge: EdgeId, target: ManagedId) {
                    self.0 = Some(target);
                }
                fn weak(&mut self, _edge: EdgeId, _target: ManagedId) {}
                fn ephemeron(&mut self, _edge: EdgeId, _key: ManagedId, _value: ManagedId) {}
            }
            let mut edge = FirstStrong(None);
            snapshot.visit_edges(root.id(), &mut edge).unwrap();
            edge.0.unwrap()
        })
        .unwrap();
    let teardown = arena.teardown();
    let target = Datum::String(target.allocation_ordinal().to_string());
    let mut observation = success(node(
        "mutation/graph",
        vec![
            ("target", target.clone()),
            (
                "objects",
                Datum::String(safepoint.objects.len().to_string()),
            ),
        ],
    ));
    observation.receipts = BoundedLane::Complete(vec![
        node(
            "mutation/safepoint",
            vec![
                ("sequence", Datum::String(safepoint.sequence.to_string())),
                ("edge-target", target),
            ],
        ),
        node(
            "mutation/teardown",
            vec![
                ("objects", Datum::String(teardown.objects.len().to_string())),
                ("roots", Datum::String(teardown.roots.len().to_string())),
            ],
        ),
    ]);
    CharacterizationCapture::new(projection_symbol(), observation)
}

fn neutral_spec(name: &str, input_limit: usize, observation_limit: usize) -> ScenarioSpec {
    ScenarioSpec::new(
        Symbol::qualified("scenario", name),
        Symbol::qualified("setup", "neutral-organs/v1"),
    )
    .with_limits(ScenarioLimits::new(input_limit, observation_limit))
    .observing(ScenarioObservationLane::ValueOrFailure)
}

fn success(value: Datum) -> CanonicalObservation {
    CanonicalObservation {
        outcome: Some(CanonicalOutcome::Success(value)),
        events: BoundedLane::Absent,
        receipts: BoundedLane::Absent,
        browse: BoundedLane::Absent,
    }
}

fn node(tag: &str, fields: Vec<(&str, Datum)>) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("specimen", tag),
        fields: fields
            .into_iter()
            .map(|(name, value)| (Symbol::new(name), value))
            .collect(),
    }
}

fn projection_symbol() -> Symbol {
    Symbol::qualified("projection", PROJECTION)
}

fn strict_projection() -> CaptureComparisonProjection {
    CaptureComparisonProjection::new(projection_symbol())
}

fn test_cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}
