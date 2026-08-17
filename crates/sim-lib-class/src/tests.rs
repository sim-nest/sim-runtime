use std::collections::BTreeSet;

use super::*;

use sim_kernel::{
    Cx, DefaultFactory, Expr, MatchScore, NoopEvalPolicy, Shape, ShapeDoc, ShapeMatch, Symbol,
    Value,
};
use std::sync::Arc;

struct AnyShape;
impl Shape for AnyShape {
    fn check_value(&self, _cx: &mut Cx, _value: Value) -> sim_kernel::Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(1)))
    }
    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> sim_kernel::Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(1)))
    }
    fn describe(&self, _cx: &mut Cx) -> sim_kernel::Result<ShapeDoc> {
        Ok(ShapeDoc::new("any"))
    }
}

fn descriptor_class(cx: &mut Cx, id: u32, name: &str, parents: Vec<DeclaredParent>) -> Value {
    let shape = cx.factory().opaque(Arc::new(AnyShape)).unwrap();
    let descriptor = ClassDescriptor::new(ClassDescriptorInput {
        identity: ClassIdentity::checked(sim_kernel::ClassId(id), Symbol::qualified("test", name))
            .unwrap(),
        parents,
        constructor_shape: shape.clone(),
        instance_shape: shape,
        members: Vec::new(),
        read_construction: None,
        metadata: Vec::new(),
    })
    .unwrap();
    cx.factory()
        .opaque(Arc::new(DescriptorClass::new(
            descriptor,
            |cx: &mut Cx, _| cx.factory().string("constructed".into()),
            32,
            32,
        )))
        .unwrap()
}

#[test]
fn descriptor_projects_kernel_class_and_bounded_subclass_evidence() {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    let root = descriptor_class(&mut cx, 7000, "Root", Vec::new());
    let root_identity =
        ClassIdentity::checked(sim_kernel::ClassId(7000), Symbol::qualified("test", "Root"))
            .unwrap();
    let leaf = descriptor_class(
        &mut cx,
        7001,
        "Leaf",
        vec![DeclaredParent::resolved(root_identity, root.clone())],
    );
    let leaf_class = leaf.object().as_class().unwrap();
    assert_eq!(leaf_class.symbol(), Symbol::qualified("test", "Leaf"));
    assert_eq!(
        leaf.object()
            .as_callable()
            .unwrap()
            .call(&mut cx, sim_kernel::Args::default())
            .unwrap()
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::String("constructed".into())
    );

    let descriptor = leaf.object().downcast_ref::<DescriptorClass>().unwrap();
    assert!(matches!(
        descriptor
            .query_subclass(&mut cx, root.clone(), 2, 1)
            .unwrap(),
        SubclassQuery::Answer {
            is_subclass: true,
            evidence: SubclassEvidence {
                visited_nodes: 2,
                performed_work: 1
            }
        }
    ));
    assert!(matches!(
        descriptor.query_subclass(&mut cx, root, 1, 1).unwrap(),
        SubclassQuery::NodeBudgetExhausted {
            limit: 1,
            required: 2,
            evidence: SubclassEvidence {
                visited_nodes: 1,
                performed_work: 1
            }
        }
    ));
}

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
