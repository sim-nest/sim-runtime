use std::sync::Arc;

use sim_codec::{Input, decode_with_codec};
use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Expr, ReadPolicy, Symbol};

use crate::{LogicDb, codec::consult_expr};

fn cx() -> Cx {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let lisp = sim_codec_lisp::LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
    cx.load_lib(&lisp).unwrap();
    cx
}

fn load_example(path: &str) -> Expr {
    decode_with_codec(
        &mut cx(),
        &Symbol::qualified("codec", "lisp"),
        Input::Text(path.to_owned()),
        ReadPolicy::default(),
    )
    .unwrap()
}

#[test]
fn kinship_example_decodes_and_consults() {
    let mut db = LogicDb::new();
    let count = consult_expr(
        &mut db,
        load_example(include_str!("../../examples/logic/kinship.siml")),
    )
    .unwrap();
    assert_eq!(count, 4);
    assert!(db.predicate_exists(&Symbol::new("parent")));
}

#[test]
fn graph_example_decodes_and_consults() {
    let mut db = LogicDb::new();
    let count = consult_expr(
        &mut db,
        load_example(include_str!("../../examples/logic/graph.siml")),
    )
    .unwrap();
    assert_eq!(count, 5);
    assert!(db.predicate_exists(&Symbol::new("edge")));
}

#[test]
fn constraints_example_decodes_and_consults() {
    let mut db = LogicDb::new();
    let count = consult_expr(
        &mut db,
        load_example(include_str!("../../examples/logic/constraints.siml")),
    )
    .unwrap();
    assert_eq!(count, 3);
    assert!(db.predicate_exists(&Symbol::new("target")));
}

#[test]
fn agent_tool_example_decodes_and_consults() {
    let mut db = LogicDb::new();
    let count = consult_expr(
        &mut db,
        load_example(include_str!("../../examples/logic/agent-tools.siml")),
    )
    .unwrap();
    assert_eq!(count, 1);
}
