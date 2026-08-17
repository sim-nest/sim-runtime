// conformance: neutral class descriptors support complete language-independent specimens.

//! End-to-end class specimens for a language that does not exist.

use std::{collections::BTreeMap, sync::Arc};

use sim_kernel::{
    Class, ClassId, Cx, Expr, MatchScore, Object, ObjectCompat, ReadConstructor, Result, Shape,
    ShapeDoc, ShapeMatch, Symbol, Value,
};
use sim_lib_class::{
    C3Policy, CacheAccessKind, ClassCache, ClassDescriptor, ClassDescriptorInput, ClassIdentity,
    DescriptorClass, LineageBudget, LineageError, LineageGraph, LineagePolicy, MemberShape,
    ReadConstruction,
};
use sim_lib_gc_tracing::CollectionLimits;

#[derive(Default)]
struct SpecimenGraph(BTreeMap<&'static str, Vec<&'static str>>);

impl LineageGraph for SpecimenGraph {
    type Node = &'static str;

    fn declared_parents(&self, node: &Self::Node) -> Vec<Self::Node> {
        self.0.get(node).cloned().unwrap_or_default()
    }
}

fn generous_budget() -> LineageBudget {
    LineageBudget {
        nodes: 32,
        work: 512,
    }
}

struct NamedShape(&'static str);

impl Shape for NamedShape {
    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(1)))
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(1)))
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new(self.0))
    }
}

struct PairReadConstructor {
    args_shape: Value,
}

impl Object for PairReadConstructor {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<read-constructor specimen:Pair>".into())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for PairReadConstructor {
    fn as_read_constructor(&self) -> Option<&dyn ReadConstructor> {
        Some(self)
    }
}

impl ReadConstructor for PairReadConstructor {
    fn symbol(&self) -> Symbol {
        Symbol::qualified("specimen", "Pair")
    }

    fn args_shape(&self, _cx: &mut Cx) -> Result<Value> {
        Ok(self.args_shape.clone())
    }

    fn construct_read(&self, cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
        cx.factory().list(args)
    }
}

fn shape(cx: &Cx, name: &'static str) -> Value {
    cx.factory().opaque(Arc::new(NamedShape(name))).unwrap()
}

#[test]
fn diamond_linearization_matches_a_hand_written_order() {
    let graph = SpecimenGraph(BTreeMap::from([
        ("Root", vec![]),
        ("Left", vec!["Root"]),
        ("Right", vec!["Root"]),
        ("Leaf", vec!["Left", "Right"]),
    ]));

    // This oracle is deliberately authored here rather than derived from C3Policy.
    let expected = vec!["Leaf", "Left", "Right", "Root"];
    assert_eq!(
        C3Policy
            .linearize(&graph, &"Leaf", generous_budget())
            .unwrap(),
        expected
    );
}

#[test]
fn inconsistent_precedence_cycle_and_budget_exhaustion_are_typed() {
    let conflict = SpecimenGraph(BTreeMap::from([
        ("X", vec![]),
        ("Y", vec![]),
        ("A", vec!["X", "Y"]),
        ("B", vec!["Y", "X"]),
        ("Z", vec!["A", "B"]),
    ]));
    assert!(matches!(
        C3Policy.linearize(&conflict, &"Z", generous_budget()),
        Err(LineageError::ConflictingPrecedence { .. })
    ));

    let cycle = SpecimenGraph(BTreeMap::from([
        ("A", vec!["B"]),
        ("B", vec!["C"]),
        ("C", vec!["A"]),
    ]));
    assert_eq!(
        C3Policy.linearize(&cycle, &"A", generous_budget()),
        Err(LineageError::Cycle {
            path: vec!["A", "B", "C", "A"]
        })
    );

    let chain = SpecimenGraph(BTreeMap::from([
        ("Root", vec![]),
        ("Middle", vec!["Root"]),
        ("Leaf", vec!["Middle"]),
    ]));
    assert_eq!(
        C3Policy.linearize(&chain, &"Leaf", LineageBudget { nodes: 2, work: 32 }),
        Err(LineageError::NodeBudgetExhausted {
            limit: 2,
            required: 3
        })
    );
}

#[test]
fn managed_cache_reclamation_observes_the_clearing_receipt() {
    let mut cache = ClassCache::new(8).unwrap();
    let root = cache.allocate_class(&[], vec!["root-member"]).unwrap();
    let leaf = cache.allocate_class(&[root], vec!["leaf-member"]).unwrap();
    let access = cache.derived(leaf, &C3Policy, generous_budget()).unwrap();
    assert_eq!(access.kind, CacheAccessKind::Recomputed);
    assert_eq!(access.view.members, ["leaf-member", "root-member"]);

    cache.release(root).unwrap();
    cache.release(leaf).unwrap();
    let receipt = cache
        .collect(CollectionLimits {
            objects: 8,
            edges: 8,
            stack: 8,
            work: 64,
            clears: 8,
            finalizers: 0,
        })
        .unwrap();
    assert_eq!(receipt.cleared_ephemerons.len(), 1);
    assert_eq!(receipt.swept.len(), 3);
    assert_eq!(cache.managed_len(), 1);
}

#[test]
fn member_shapes_are_browseable_and_read_construction_round_trips() {
    let mut cx = sim_kernel::testing::bare_cx();
    let any = shape(&cx, "specimen:any");
    let coordinate = shape(&cx, "specimen:coordinate");
    let read_constructor = cx
        .factory()
        .opaque(Arc::new(PairReadConstructor {
            args_shape: coordinate.clone(),
        }))
        .unwrap();
    let descriptor = ClassDescriptor::new(ClassDescriptorInput {
        identity: ClassIdentity::checked(ClassId(19_005), Symbol::qualified("specimen", "Pair"))
            .unwrap(),
        parents: vec![],
        constructor_shape: any.clone(),
        instance_shape: any,
        members: vec![MemberShape {
            name: Symbol::new("first"),
            shape: coordinate.clone(),
        }],
        read_construction: Some(ReadConstruction {
            constructor: read_constructor,
            args_shape: coordinate.clone(),
        }),
        metadata: vec![],
    })
    .unwrap();
    let class = DescriptorClass::new(descriptor, |cx: &mut Cx, _| cx.factory().nil(), 8, 32);

    let members = class.members(&mut cx).unwrap();
    let browsed = members
        .object()
        .as_table_impl()
        .unwrap()
        .get(&mut cx, Symbol::new("first"))
        .unwrap();
    assert_eq!(
        browsed
            .object()
            .as_shape()
            .unwrap()
            .describe(&mut cx)
            .unwrap()
            .name,
        "specimen:coordinate"
    );

    let constructor = class.read_constructor(&mut cx).unwrap().unwrap();
    assert_eq!(
        constructor
            .object()
            .as_read_constructor()
            .unwrap()
            .args_shape(&mut cx)
            .unwrap(),
        coordinate
    );
    let args = vec![
        cx.factory().string("left".into()).unwrap(),
        cx.factory().string("right".into()).unwrap(),
    ];
    let reconstructed = constructor
        .object()
        .as_read_constructor()
        .unwrap()
        .construct_read(&mut cx, args.clone())
        .unwrap();
    let observed = sim_kernel::force_list_to_vec(
        &mut cx,
        reconstructed.object().as_list().unwrap(),
        "specimen Pair read construction",
    )
    .unwrap();
    assert_eq!(observed, args);
}

#[test]
fn specimen_has_no_guest_language_dependency_or_type() {
    let manifest = include_str!("../Cargo.toml");
    let source = include_str!("neutral_class_specimens.rs");
    assert!(!manifest.contains("sim-lib-lang-"));
    assert!(!source.contains(&["sim", "_lib_lang_"].concat()));
}
