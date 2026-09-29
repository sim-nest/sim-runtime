use crate::*;
use sim_kernel::{CapabilityName, Datum, Symbol};
use sim_lib_journal::MemoryBackend;

fn identity(reverse: bool) -> Datum {
    let mut members = vec![Datum::String("a".into()), Datum::String("b".into())];
    if reverse {
        members.reverse();
    }
    let mut fields = vec![
        (Symbol::new("z"), Datum::Set(members)),
        (Symbol::new("a"), Datum::String("same-authority".into())),
    ];
    if reverse {
        fields.reverse();
    }
    Datum::Node {
        tag: Symbol::qualified("fixture", "identity"),
        fields,
    }
}

#[test]
fn semantic_identity_preserves_ordered_values_and_refuses_invalid_collections() {
    use crate::operation_wire::same_datum;
    assert!(same_datum(&identity(false), &identity(true)));
    let ordered = Datum::List(vec![Datum::Bool(true), Datum::Bool(false)]);
    let changed = Datum::List(vec![Datum::Bool(false), Datum::Bool(true)]);
    assert!(!same_datum(&ordered, &changed));
    for invalid in [
        Datum::Set(vec![Datum::Nil, Datum::Nil]),
        Datum::Map(vec![
            (Datum::Nil, Datum::Bool(true)),
            (Datum::Nil, Datum::Bool(false)),
        ]),
        Datum::Node {
            tag: Symbol::new("fixture"),
            fields: vec![
                (Symbol::new("a"), Datum::Nil),
                (Symbol::new("a"), Datum::Nil),
            ],
        },
    ] {
        assert!(!same_datum(&invalid, &invalid));
    }
    let intent = OperationIntent::new(
        "fixture/check",
        identity(false),
        ordered,
        ReplayPolicy::ExactlyOnce,
    )
    .unwrap();
    let mut reordered = intent.canonical_datum();
    if let Datum::Node { fields, .. } = &mut reordered {
        fields.reverse();
    }
    assert_eq!(
        OperationIntent::from_datum(&reordered).unwrap().id(),
        intent.id()
    );
}

#[test]
fn reordered_same_authority_is_not_an_independent_observer() {
    struct Performer;
    impl LifecyclePerformer for Performer {
        fn identity(&self) -> Datum {
            identity(false)
        }
        fn perform(&mut self, _: &FencedDispatch) -> LifecyclePerformerResponse {
            panic!("not independent")
        }
    }
    struct Observer;
    impl PostconditionObserver for Observer {
        fn identity(&self) -> Datum {
            identity(true)
        }
        fn observe(&mut self, _: &PostconditionRequest) -> PostconditionResponse {
            panic!("not independent")
        }
    }
    let mut lifecycle = OperationLifecycle::new(MemoryBackend::new());
    let intent = OperationIntent::new(
        "fixture/check",
        Datum::Nil,
        Datum::Nil,
        ReplayPolicy::ExactlyOnce,
    )
    .unwrap();
    let grant = OperationGrant::new(
        intent.id().clone(),
        CapabilityName::new("fixture/check"),
        Datum::String("fixture/authority".into()),
    )
    .unwrap();
    let result = lifecycle.run(
        &intent,
        &grant,
        LeaseWindow::new(Datum::String("fixture/worker".into()), 1, 2).unwrap(),
        &mut Performer,
        &mut Observer,
    );
    assert!(matches!(
        result,
        Err(OperationError::ObserverNotIndependent)
    ));
    let record = lifecycle.record(intent.id()).unwrap().unwrap();
    assert!(record.observations().is_empty());
    assert!(record.dispatches().is_empty());
}
