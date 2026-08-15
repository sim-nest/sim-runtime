// conformance: incremental query organ callables, cutoff, reports, and Card projection

use sim_kernel::{
    Args, Cx, Error, Expr, Ref, Symbol, Value,
    card::{card_for_ref, card_kind_predicate},
    force_list_to_vec,
    standard::standard_organ_kind,
};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

use sim_incremental_core::{
    QueryBudgets, ValueFingerprint,
    dataflow::{
        AdmittedTransfer, Boundary, DataflowGraph, EdgeClass, EdgeSpec, FixpointEngine,
        GraphDirection, JoinSemilattice, NodeSpec, StateSize, TransferPolicy,
    },
};

fn string(cx: &mut Cx, value: &str) -> Value {
    cx.factory().string(value.to_owned()).unwrap()
}

fn expr_value(cx: &mut Cx, expr: Expr) -> Value {
    cx.factory().expr(expr).unwrap()
}

fn call(cx: &mut Cx, name: &str, args: Vec<Value>) -> Value {
    let function = cx
        .resolve_function(&Symbol::qualified("incremental", name))
        .unwrap();
    let callable = function.object().as_callable().unwrap();
    callable.call(cx, Args::new(args)).unwrap()
}

fn call_err(cx: &mut Cx, name: &str, args: Vec<Value>) -> Error {
    let function = cx
        .resolve_function(&Symbol::qualified("incremental", name))
        .unwrap();
    let callable = function.object().as_callable().unwrap();
    callable.call(cx, Args::new(args)).unwrap_err()
}

fn register_query(cx: &mut Cx, engine: &Value, key: &str, source: Expr) -> Value {
    let key = string(cx, key);
    let source = expr_value(cx, source);
    call(cx, "register", vec![engine.clone(), key, source])
}

fn verify_query(cx: &mut Cx, engine: &Value, key: &str) -> Value {
    let key = string(cx, key);
    call(cx, "verify", vec![engine.clone(), key])
}

fn invalidate_key(cx: &mut Cx, engine: &Value, key: &str) -> Value {
    let key = string(cx, key);
    call(cx, "invalidate", vec![engine.clone(), key])
}

fn report(cx: &mut Cx, name: &str, engine: &Value) -> Value {
    call(cx, name, vec![engine.clone()])
}

fn keyed_report(cx: &mut Cx, name: &str, engine: &Value, key: &str) -> Value {
    let key = string(cx, key);
    call(cx, name, vec![engine.clone(), key])
}

fn table_get(cx: &mut Cx, table_value: &Value, key: &str) -> Value {
    table_value
        .object()
        .as_table_impl()
        .unwrap()
        .get(cx, Symbol::new(key))
        .unwrap()
}

fn number_text(cx: &mut Cx, value: &Value) -> String {
    match value.object().as_expr(cx).unwrap() {
        Expr::Number(number) => number.canonical,
        other => panic!("expected number, got {other:?}"),
    }
}

fn grant_all(cx: &mut Cx) {
    cx.grant(incremental_read_capability());
    cx.grant(incremental_write_capability());
    cx.grant(incremental_verify_capability());
}

fn read_expr(key: &str) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("incremental", "read"))),
        args: vec![Expr::String(key.to_owned())],
    }
}

fn missing_expr(key: &str) -> Expr {
    Expr::Call {
        operator: Box::new(Expr::Symbol(Symbol::qualified("incremental", "missing"))),
        args: vec![Expr::String(key.to_owned())],
    }
}

#[test]
fn callables_have_shape_contracts_and_capability_gates() {
    let mut cx = cx();
    install_incremental_lib(&mut cx).unwrap();
    let engine = call(&mut cx, "engine", Vec::new());

    for name in [
        "engine",
        "register",
        "invalidate",
        "verify",
        "explain",
        "snapshot",
        "metrics",
    ] {
        let function = cx
            .resolve_function(&Symbol::qualified("incremental", name))
            .unwrap();
        let callable = function.object().as_callable().unwrap();
        assert!(
            callable.browse_args_shape(&mut cx).unwrap().is_some(),
            "{name}"
        );
        assert!(
            callable.browse_result_shape(&mut cx).unwrap().is_some(),
            "{name}"
        );
    }

    let key = string(&mut cx, "leaf");
    let source = expr_value(&mut cx, Expr::String("same".to_owned()));
    let denied = call_err(&mut cx, "register", vec![engine.clone(), key, source]);
    assert!(
        matches!(denied, Error::CapabilityDenied { capability } if capability == incremental_write_capability())
    );

    cx.grant(incremental_write_capability());
    register_query(&mut cx, &engine, "leaf", Expr::String("same".to_owned()));

    let key = string(&mut cx, "leaf");
    let denied = call_err(&mut cx, "verify", vec![engine, key]);
    assert!(
        matches!(denied, Error::CapabilityDenied { capability } if capability == incremental_verify_capability())
    );
}

#[test]
fn query_family_tracks_missing_invalidation_and_cutoff() {
    let mut cx = cx();
    install_incremental_lib(&mut cx).unwrap();
    grant_all(&mut cx);
    let engine = call(&mut cx, "engine", Vec::new());
    register_query(&mut cx, &engine, "leaf", Expr::String("same".to_owned()));
    register_query(&mut cx, &engine, "root", read_expr("leaf"));
    register_query(
        &mut cx,
        &engine,
        "guard",
        Expr::Vector(vec![read_expr("root"), missing_expr("optional")]),
    );

    let verified = verify_query(&mut cx, &engine, "guard");
    assert_eq!(
        verified.object().as_expr(&mut cx).unwrap(),
        Expr::Vector(vec![Expr::String("same".to_owned()), Expr::Bool(false)])
    );
    let metrics = report(&mut cx, "metrics", &engine);
    let executions = table_get(&mut cx, &metrics, "executions");
    let leaf_count = table_get(&mut cx, &executions, "leaf");
    assert_eq!(number_text(&mut cx, &leaf_count), "1");
    let root_count = table_get(&mut cx, &executions, "root");
    assert_eq!(number_text(&mut cx, &root_count), "1");
    let guard_count = table_get(&mut cx, &executions, "guard");
    assert_eq!(number_text(&mut cx, &guard_count), "1");

    verify_query(&mut cx, &engine, "guard");
    let metrics = report(&mut cx, "metrics", &engine);
    let executions = table_get(&mut cx, &metrics, "executions");
    let guard_count = table_get(&mut cx, &executions, "guard");
    assert_eq!(number_text(&mut cx, &guard_count), "1");

    invalidate_key(&mut cx, &engine, "optional");
    verify_query(&mut cx, &engine, "guard");
    let metrics = report(&mut cx, "metrics", &engine);
    let executions = table_get(&mut cx, &metrics, "executions");
    let guard_count = table_get(&mut cx, &executions, "guard");
    assert_eq!(number_text(&mut cx, &guard_count), "2");
    let root_count = table_get(&mut cx, &executions, "root");
    assert_eq!(number_text(&mut cx, &root_count), "1");
}

#[test]
fn explanation_snapshot_and_card_projection_are_browseable() {
    let mut cx = cx();
    install_incremental_lib(&mut cx).unwrap();
    grant_all(&mut cx);
    let engine = call(&mut cx, "engine", Vec::new());
    register_query(&mut cx, &engine, "leaf", Expr::String("same".to_owned()));
    register_query(&mut cx, &engine, "root", read_expr("leaf"));
    verify_query(&mut cx, &engine, "root");

    let explain = keyed_report(&mut cx, "explain", &engine, "root");
    assert_eq!(
        table_get(&mut cx, &explain, "registered")
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::Bool(true)
    );
    assert_ne!(
        table_get(&mut cx, &explain, "fingerprint")
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::Nil
    );

    let snapshot = keyed_report(&mut cx, "snapshot", &engine, "root");
    let nodes = table_get(&mut cx, &snapshot, "nodes");
    let list = nodes.object().as_list().unwrap();
    let nodes = force_list_to_vec(&mut cx, list, "incremental snapshot nodes").unwrap();
    assert_eq!(nodes.len(), 2);

    let claims = cx
        .query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(incremental_organ_symbol())),
            predicate: Some(card_kind_predicate()),
            object: Some(Ref::Symbol(standard_organ_kind())),
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(claims.len(), 1);
    let card = card_for_ref(&mut cx, Ref::Symbol(incremental_organ_symbol())).unwrap();
    let table = card.object().as_table(&mut cx).unwrap();
    let entries = table.object().as_table_impl().unwrap();
    let ops = entries.get(&mut cx, Symbol::new("ops")).unwrap();
    let list = ops.object().as_list().unwrap();
    let values = force_list_to_vec(&mut cx, list, "incremental organ ops").unwrap();
    assert!(values.into_iter().any(|value| {
        value.object().as_expr(&mut cx).unwrap()
            == Expr::Symbol(Symbol::qualified("incremental", "register.v1"))
    }));
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct TestFacts(u8);

impl StateSize for TestFacts {
    fn state_size(&self) -> usize {
        1
    }
}

impl JoinSemilattice for TestFacts {
    fn bottom(&self) -> Self {
        Self(0)
    }

    fn join(&self, other: &Self) -> Self {
        Self(self.0 | other.0)
    }

    fn less_equal(&self, other: &Self) -> bool {
        self.0 & other.0 == self.0
    }
}

#[derive(Clone)]
struct IdentityTransfer;

impl TransferPolicy<TestFacts> for IdentityTransfer {
    fn fingerprint(&self) -> ValueFingerprint {
        ValueFingerprint::new(19)
    }

    fn policy_size(&self) -> usize {
        0
    }

    fn transfer(&self, state: &TestFacts) -> TestFacts {
        state.clone()
    }
}

#[test]
fn completed_analysis_projects_browsable_boundary_states_and_proof_identity() {
    let graph = DataflowGraph::build(
        [
            NodeSpec {
                id: "entry".to_owned(),
                location: "recipe.siml:1".to_owned(),
                boundary: Boundary::Input,
            },
            NodeSpec {
                id: "exit".to_owned(),
                location: "recipe.siml:2".to_owned(),
                boundary: Boundary::Output,
            },
        ],
        [EdgeSpec {
            id: "flow".to_owned(),
            source: "entry".to_owned(),
            target: "exit".to_owned(),
            class: EdgeClass::<String>::Data,
            direction: GraphDirection::Forward,
        }],
    )
    .unwrap();
    let transfer = AdmittedTransfer::admit(
        IdentityTransfer,
        &[TestFacts(0), TestFacts(1), TestFacts(3)],
    )
    .unwrap();
    let proof = FixpointEngine::solve_proven(
        &graph,
        &transfer,
        TestFacts(0),
        [("entry".to_owned(), TestFacts(1))],
        QueryBudgets::default(),
    )
    .unwrap();
    let expected_identity = proof.identity().get().to_string();
    let mut cx = cx();
    install_incremental_lib(&mut cx).unwrap();
    let analysis = DataflowAnalysisView::from_completion(&mut cx, &graph, proof, |state| {
        Expr::Number(sim_kernel::NumberLiteral {
            domain: Symbol::qualified("core", "u8"),
            canonical: state.0.to_string(),
        })
    })
    .unwrap();
    let value = cx.factory().opaque(std::sync::Arc::new(analysis)).unwrap();

    let inputs = table_get(&mut cx, &value, "input-states");
    assert!(
        !inputs
            .object()
            .as_dir()
            .unwrap()
            .is_dir(&mut cx, Symbol::new("entry"))
            .unwrap()
    );
    assert_ne!(
        table_get(&mut cx, &inputs, "entry")
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::Nil
    );
    assert_eq!(
        table_get(&mut cx, &inputs, "exit")
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::Nil
    );

    let outputs = table_get(&mut cx, &value, "output-states");
    assert_ne!(
        table_get(&mut cx, &outputs, "exit")
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::Nil
    );
    assert_eq!(
        table_get(&mut cx, &outputs, "entry")
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::Nil
    );

    let proof = table_get(&mut cx, &value, "proof");
    let fingerprint = table_get(&mut cx, &proof, "fingerprint");
    assert_eq!(number_text(&mut cx, &fingerprint), expected_identity);
    let shape = cx
        .resolve_shape(&incremental_analysis_shape_symbol())
        .unwrap();
    assert!(
        shape
            .object()
            .as_shape()
            .unwrap()
            .check_value(&mut cx, value)
            .unwrap()
            .accepted
    );
}
