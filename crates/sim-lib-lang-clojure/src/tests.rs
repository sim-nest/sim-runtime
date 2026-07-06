use std::sync::Arc;

use sim_codec::{Input, decode_tree_with_codec};
use sim_kernel::{
    Cx, Expr, NumberLiteral, ReadPolicy, Ref, Symbol, TrustLevel, Value,
    capability::{control_capture_capability, control_prompt_capability},
    control::{control_aborted_status, control_result_status},
};
use sim_lib_namespace::{ImportOptions, Namespace};
use sim_lib_sequence::{TransducerPipeline, force_sequence_bounded};
use sim_lib_standard_core::ProfileRegistry;

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn number(cx: &mut Cx, value: u64) -> Value {
    cx.factory()
        .number_literal(Symbol::qualified("test", "u64"), value.to_string())
        .unwrap()
}

fn number_from_value(cx: &mut Cx, value: &Value) -> u64 {
    let Expr::Number(NumberLiteral { canonical, .. }) = value.object().as_expr(cx).unwrap() else {
        panic!("expected number value");
    };
    canonical.parse().unwrap()
}

#[test]
fn edn_reader_decodes_core_data_with_locations() {
    let mut cx = cx();
    let codec_id = cx.registry_mut().fresh_codec_id();
    cx.load_lib(&ClojureEdnCodecLib::new(codec_id)).unwrap();

    let tree = decode_tree_with_codec(
        &mut cx,
        &clojure_edn_reader_symbol(),
        Input::Text("{:a [1 2] :flag true}".to_owned()),
        read_policy(),
        "unit.edn",
    )
    .unwrap();

    assert_eq!(tree.origin.as_ref().unwrap().source.0.as_str(), "unit.edn");
    let Expr::Map(entries) = &tree.expr else {
        panic!("expected EDN map");
    };
    assert_eq!(entries.len(), 2);
    assert_eq!(tree.children.len(), 4);
    assert_eq!(
        entries[0].0,
        Expr::Symbol(Symbol::qualified("keyword", "a"))
    );
    assert!(matches!(entries[0].1, Expr::Vector(_)));
}

#[test]
fn edn_string_literal_preserves_non_ascii() {
    let mut cx = cx();
    let codec_id = cx.registry_mut().fresh_codec_id();
    cx.load_lib(&ClojureEdnCodecLib::new(codec_id)).unwrap();

    // Source holds a 2-byte UTF-8 scalar inside the quotes; kept ASCII-only in
    // this file via a `\u{..}` escape (R8). It must decode, not turn to mojibake.
    let tree = decode_tree_with_codec(
        &mut cx,
        &clojure_edn_reader_symbol(),
        Input::Text("\"caf\u{00e9}\"".to_owned()),
        read_policy(),
        "unicode.edn",
    )
    .unwrap();
    assert_eq!(tree.expr, Expr::String("caf\u{00e9}".to_owned()));
}

#[test]
fn clojure_data_uses_sequence_organ() {
    let mut cx = cx();
    let expr = Expr::Vector(vec![
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "i64"),
            canonical: "1".to_owned(),
        }),
        Expr::Number(NumberLiteral {
            domain: Symbol::qualified("numbers", "i64"),
            canonical: "2".to_owned(),
        }),
    ]);
    let vector = edn_expr_to_value(&mut cx, &expr).unwrap();
    assert_eq!(vector.object().as_expr(&mut cx).unwrap(), expr);

    let one = number(&mut cx, 1);
    let two = number(&mut cx, 2);
    let three = number(&mut cx, 3);
    let source = clojure_profile_sequence(&mut cx, vec![one, two, three]).unwrap();
    let pipeline = TransducerPipeline::new().map(Arc::new(|cx, value| {
        let next = number_from_value(cx, &value) + 1;
        Ok(number(cx, next))
    }));
    let zero = number(&mut cx, 0);
    let sum = clojure_transduce(
        &mut cx,
        &source,
        pipeline,
        zero,
        Arc::new(|cx, acc, value| {
            let left = number_from_value(cx, &acc);
            let right = number_from_value(cx, &value);
            Ok(number(cx, left + right))
        }),
    )
    .unwrap();

    assert_eq!(number_from_value(&mut cx, &sum), 9);
    assert!(force_sequence_bounded(&mut cx, &source, 0, "already consumed").is_ok());
}

#[test]
fn clojure_core_namespace_exports_sequence_and_recur() {
    let source = clojure_core_namespace().unwrap();
    let mut user = Namespace::module(Symbol::qualified("user", "unit"));
    user.import_from(
        &source,
        &Symbol::new("transduce"),
        ImportOptions::new().rename(Symbol::new("xduce")),
    )
    .unwrap();
    user.import_from(&source, &Symbol::new("recur"), ImportOptions::new())
        .unwrap();

    assert_eq!(
        user.resolve(&Symbol::new("xduce")).unwrap().target(),
        &Symbol::qualified("sequence", "transduce.v1")
    );
    assert_eq!(
        user.resolve(&Symbol::new("recur")).unwrap().target(),
        &Symbol::qualified("control", "abort.v1")
    );
}

#[test]
fn recur_uses_control_prompt() {
    let mut cx = cx();
    sim_lib_control::install_control_policy(&mut cx);
    cx.grant(control_prompt_capability());
    cx.grant(control_capture_capability());
    let result = clojure_loop_prompt(&mut cx, Ref::Symbol(Symbol::new("input")), |cx| {
        clojure_recur(cx, Ref::Symbol(Symbol::new("next-bindings")))
    })
    .unwrap();

    assert_eq!(
        control_result_status(&cx, &result).unwrap(),
        Some(control_aborted_status())
    );
}

#[test]
fn clojure_profile_publishes_per_organ_fidelity() {
    let mut cx = cx();
    let mut registry = ProfileRegistry::new();
    let profile = install_clojure_core_profile(&mut cx, &mut registry).unwrap();

    let badges = cx
        .query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(profile.symbol.clone())),
            predicate: Some(Symbol::qualified("standard", "fidelity-badge")),
            object: None,
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(badges.len(), 3);
    for badge in [
        clojure_sequence_fidelity_symbol(),
        clojure_namespace_fidelity_symbol(),
        clojure_control_fidelity_symbol(),
    ] {
        assert!(
            badges
                .iter()
                .any(|claim| claim.object == Ref::Symbol(badge.clone()))
        );
    }
    assert!(registry.profile(&profile.symbol).is_some());
}

fn read_policy() -> ReadPolicy {
    ReadPolicy {
        trust: TrustLevel::TrustedSource,
        capabilities: sim_kernel::CapabilitySet::new(),
    }
}
