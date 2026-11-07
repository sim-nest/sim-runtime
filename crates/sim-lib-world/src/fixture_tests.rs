// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::{Datum, Symbol};

use crate::WorldProduct;
use crate::command::render;
use crate::observe;
use crate::product::{DISCLOSURE_CONCLUSION, DISCLOSURE_FACT, SOURCE_CONCLUSION, SOURCE_FACT};
use crate::seal;

fn node<'a>(value: &'a Datum, tag: &str) -> &'a [(Symbol, Datum)] {
    match value {
        Datum::Node {
            tag: actual,
            fields,
            ..
        } if actual.as_qualified_str() == tag => fields,
        other => panic!("expected {tag}, got {other:?}"),
    }
}

fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> &'a Datum {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn field_opt<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Option<&'a Datum> {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
}

fn text(value: &Datum) -> &str {
    match value {
        Datum::String(text) => text,
        other => panic!("string, got {other:?}"),
    }
}

fn number(value: &Datum) -> u64 {
    match value {
        Datum::Number(number) => number.canonical.parse().expect("count"),
        other => panic!("number, got {other:?}"),
    }
}

fn vector(value: &Datum) -> &[Datum] {
    match value {
        Datum::Vector(values) => values,
        other => panic!("vector, got {other:?}"),
    }
}

fn strings(value: &Datum) -> Vec<String> {
    vector(value)
        .iter()
        .map(|item| text(item).to_owned())
        .collect()
}

type FieldList = Vec<(Symbol, Datum)>;

fn observation_row(batch: &str, selector: &str) -> (FieldList, FieldList) {
    let value = observe::snapshot(batch, selector).expect("snapshot");
    let fields = node(&value, "world/snapshot-v1").to_vec();
    let row = node(
        &vector(field(&fields, "observations"))[0],
        "world/observation-v1",
    )
    .to_vec();
    (fields, row)
}

#[test]
fn caller_named_batch_reads_only_its_provider() {
    let (fields, row) = observation_row("lantern", "provider/widget");
    assert_eq!(text(field(&fields, "batch")), "lantern");
    assert_eq!(strings(field(&fields, "reads")), vec!["provider/widget"]);
    assert_eq!(strings(field(&fields, "ports")), vec!["provider"]);
    assert_eq!(number(field(&fields, "effects")), 0);
    assert_eq!(number(field(&fields, "subprocesses")), 0);
    assert_eq!(text(field(&row, "status")), "observed");
    assert_eq!(text(field(&row, "value")), "widget-body");
    assert_eq!(text(field(&row, "port")), "provider");
    assert_eq!(
        text(field(&row, "provenance")),
        "verified-source/provider/widget"
    );
    assert_eq!(field(&row, "matched"), &Datum::Bool(true));

    let (generated_fields, generated) = observation_row("lantern", "provider/generated");
    assert_eq!(strings(field(&generated_fields, "ports")), vec!["provider"]);
    assert_eq!(text(field(&generated, "value")), "generated-widget");

    let missed = observe::snapshot("lantern", "provider/missing").expect("miss");
    let fields = node(&missed, "world/snapshot-v1");
    assert_eq!(
        strings(field(fields, "reads")),
        vec!["provider/missing".to_owned()]
    );
    assert_eq!(strings(field(fields, "ports")), vec!["provider".to_owned()]);
    let row = node(
        &vector(field(fields, "observations"))[0],
        "world/observation-v1",
    );
    assert_eq!(text(field(row, "status")), "absent");
    assert_eq!(text(field(row, "provenance")), "selector-miss");
    assert_eq!(field(row, "matched"), &Datum::Bool(false));
}

#[test]
fn undeclared_provider_is_a_gap_and_shared_names_agree() {
    let gap = observe::snapshot("lantern", "membrane/linux").expect("gap");
    let rendered = render(&gap);
    assert!(!rendered.contains("verified-source"));
    assert!(!rendered.contains("http"));
    let fields = node(&gap, "world/observation-gap-v1");
    assert_eq!(text(field(fields, "reason")), "missing-capability");
    assert_eq!(text(field(fields, "selector")), "membrane/linux");
    assert_eq!(number(field(fields, "effects")), 0);
    assert_eq!(number(field(fields, "subprocesses")), 0);

    let (left_fields, left) = observation_row("lantern", "provider/widget");
    let (right_fields, right) = observation_row("lantern-alias", "provider/widget");
    assert_eq!(text(field(&left_fields, "batch")), "lantern");
    assert_eq!(text(field(&right_fields, "batch")), "lantern-alias");
    assert_eq!(
        text(field(&left, "semantic-id")),
        text(field(&right, "semantic-id"))
    );
    assert_eq!(text(field(&left, "value")), text(field(&right, "value")));

    let same = observe::diff("lantern", "lantern-alias", "provider/widget").expect("same");
    let fields = node(&same, "world/observation-diff-v1");
    assert_eq!(field(fields, "changed"), &Datum::Bool(false));
    assert!(strings(field(fields, "reads")).is_empty());
    assert!(strings(field(fields, "ports")).is_empty());

    let changed = observe::diff("lantern", "lantern-revised", "provider/widget").expect("changed");
    let fields = node(&changed, "world/observation-diff-v1");
    assert_eq!(field(fields, "changed"), &Datum::Bool(true));
    assert_eq!(text(field(fields, "before-value")), "widget-body");
    assert_eq!(text(field(fields, "after-value")), "widget-next");
    assert_eq!(
        strings(field(fields, "reads")),
        vec!["provider/widget".to_owned(), "provider/widget".to_owned()]
    );
    assert_eq!(
        strings(field(fields, "ports")),
        vec!["provider".to_owned(), "provider".to_owned()]
    );
}

#[test]
fn hostile_remote_stays_unavailable_and_uncontacted() {
    let (fields, row) = observation_row("lantern", "remote/hostile");
    let rendered = render(&observe::snapshot("lantern", "remote/hostile").expect("remote"));
    assert!(!rendered.contains("http"));
    assert!(!rendered.contains("verified-source"));
    assert_eq!(text(field(&row, "status")), "unavailable");
    assert!(field_opt(&row, "value").is_none());
    assert_eq!(text(field(&row, "provenance")), "model/remote/hostile");
    assert_eq!(field(&row, "matched"), &Datum::Bool(true));
    assert_eq!(strings(field(&fields, "ports")), vec!["remote"]);
    assert_eq!(strings(field(&fields, "reads")), vec!["remote/hostile"]);
    assert_eq!(number(field(&fields, "effects")), 0);
    assert_eq!(number(field(&fields, "subprocesses")), 0);

    let same = observe::diff("lantern", "lantern-revised", "remote/hostile").expect("same");
    let fields = node(&same, "world/observation-diff-v1");
    assert_eq!(field(fields, "changed"), &Datum::Bool(false));
    assert!(strings(field(fields, "reads")).is_empty());
}

#[test]
fn proofs_explain_the_named_owner_and_leave_core_owners_in_place() {
    let product = WorldProduct::bundled().expect("product");
    let projected = product
        .project(
            "world/generated-ownership-v1",
            "fact/lantern-generated",
            Datum::String("generated-widget".to_owned()),
            None,
        )
        .expect("project");
    let fields = node(&projected.value, "world/projection-v1");
    assert_eq!(
        strings(field(fields, "affected")),
        vec!["conclusion/lantern-proof".to_owned()]
    );
    assert!(render(&projected.value).contains("generated-widget"));
    assert_eq!(product.effect_calls(), 0);

    let explained = product
        .why("conclusion/lantern-proof", "fact/lantern-generated")
        .expect("why");
    let path = strings(field(node(&explained, "world/explanation-v1"), "path"));
    assert_eq!(
        path,
        vec![
            "owner/lantern/widget".to_owned(),
            "conclusion/conclusion/lantern-proof".to_owned(),
            "fact/fact/lantern-generated".to_owned(),
        ]
    );

    let source = product
        .why(SOURCE_CONCLUSION, SOURCE_FACT)
        .expect("source why");
    assert_eq!(
        strings(field(node(&source, "world/explanation-v1"), "path"))[0],
        "owner/sim-runtime/sim-lib-world"
    );
    let disclosure = product
        .why(DISCLOSURE_CONCLUSION, DISCLOSURE_FACT)
        .expect("disclosure why");
    assert_eq!(
        strings(field(node(&disclosure, "world/explanation-v1"), "path"))[0],
        "owner/sim-private/disclosure"
    );

    let changed = product
        .diff(
            "world/package-assembly-v1",
            "fact/lantern-source",
            Datum::String("source-a".to_owned()),
            Datum::String("source-b".to_owned()),
        )
        .expect("diff");
    assert_eq!(
        strings(field(node(&changed, "world/diff-v1"), "affected")),
        vec!["conclusion/lantern-proof".to_owned()]
    );
    let disclosure = product
        .diff(
            "no-v3/disclosure-policy-v1",
            DISCLOSURE_FACT,
            Datum::String("allowlist/a".to_owned()),
            Datum::String("allowlist/b".to_owned()),
        )
        .expect("disclosure diff");
    assert_eq!(
        strings(field(node(&disclosure, "world/diff-v1"), "affected")),
        vec![DISCLOSURE_CONCLUSION.to_owned()]
    );
}

#[test]
fn nested_source_admits_and_a_stub_refuses_before_compile() {
    let admitted = seal::open("lantern", None).expect("seal");
    let fields = node(&admitted, "world/source-seal-v1");
    assert_eq!(text(field(fields, "admission")), "admit");
    assert!(field_opt(fields, "reason").is_none());
    assert_eq!(field(fields, "compiler-opened"), &Datum::Bool(false));
    assert_eq!(number(field(fields, "domains")), 2);
    assert_eq!(number(field(fields, "members")), 2);
    assert_eq!(number(field(fields, "generated")), 1);
    assert_eq!(number(field(fields, "verified")), 2);
    assert_eq!(number(field(fields, "effects")), 0);
    assert_eq!(number(field(fields, "subprocesses")), 0);

    let alias = seal::open("lantern-alias", None).expect("alias");
    let alias_fields = node(&alias, "world/source-seal-v1");
    assert_eq!(
        text(field(fields, "digest")),
        text(field(alias_fields, "digest"))
    );
    assert_ne!(
        text(field(fields, "batch")),
        text(field(alias_fields, "batch"))
    );

    let universe = seal::universe("lantern", None).expect("universe");
    let fields = node(&universe, "world/source-universe-v1");
    let domains = vector(field(fields, "domain-rows"));
    let ids = domains
        .iter()
        .map(|row| text(field(node(row, "world/resolver-domain-v1"), "id")).to_owned())
        .collect::<Vec<_>>();
    assert!(ids.contains(&"resolver/lantern".to_owned()));
    assert!(ids.contains(&"resolver/lantern/vendor/widgets".to_owned()));
    let members = vector(field(fields, "member-rows"));
    let sources = members
        .iter()
        .map(|row| text(field(node(row, "world/source-member-v1"), "source")).to_owned())
        .collect::<Vec<_>>();
    assert!(sources.contains(&"crates/lantern".to_owned()));
    assert!(sources.contains(&"vendor/widgets/widget".to_owned()));
    assert_ne!(sources[0], sources[1]);
    let generated = vector(field(fields, "generated-rows"));
    assert_eq!(generated.len(), 1);
    let generated = node(&generated[0], "world/generated-input-v1");
    assert_eq!(text(field(generated, "path")), "src/generated.rs");
    assert_eq!(text(field(generated, "checksum")), "aaaa");

    let refused = seal::open("lantern-stub", None).expect("stub");
    let rendered = render(&refused);
    assert!(!rendered.contains("compiler opened"));
    let fields = node(&refused, "world/source-seal-v1");
    assert_eq!(text(field(fields, "admission")), "refuse");
    assert_eq!(text(field(fields, "reason")), "stub-only");
    assert_eq!(
        text(field(fields, "coordinate")),
        "resolver/lantern lantern 1.0.0"
    );
    assert_eq!(field(fields, "compiler-opened"), &Datum::Bool(false));
    assert_eq!(number(field(fields, "stub-only")), 1);
    assert_eq!(number(field(fields, "verified")), 1);
}

#[test]
fn one_generated_checksum_is_the_only_changed_input() {
    let changed = seal::diff("lantern", "lantern-revised", None).expect("diff");
    let fields = node(&changed, "world/source-seal-diff-v1");
    assert_eq!(field(fields, "changed"), &Datum::Bool(true));
    assert_eq!(field(fields, "compiler-opened"), &Datum::Bool(false));
    let inputs = vector(field(fields, "inputs"));
    assert_eq!(inputs.len(), 1);
    let input = node(&inputs[0], "world/changed-input-v1");
    assert_eq!(text(field(input, "field")), "checksum");
    assert_eq!(text(field(input, "origin")), "generated");
    assert_eq!(text(field(input, "name")), "src/generated.rs");
    assert_eq!(text(field(input, "domain")), "resolver/lantern");

    let same = seal::diff("lantern", "lantern-alias", None).expect("same");
    let fields = node(&same, "world/source-seal-diff-v1");
    assert_eq!(field(fields, "changed"), &Datum::Bool(false));
    assert!(vector(field(fields, "inputs")).is_empty());
}

#[test]
fn caller_named_sources_do_not_open_a_host() {
    let sources = [
        include_str!("fixture.rs"),
        include_str!("observe.rs"),
        include_str!("product.rs"),
        include_str!("command.rs"),
        include_str!("seal.rs"),
        include_str!("seal/batch.rs"),
        include_str!("seal/emit.rs"),
        include_str!("seal/parse.rs"),
        include_str!("seal/marks.rs"),
    ];
    for source in sources {
        assert!(!source.contains("std::fs"));
        assert!(!source.contains("std::process"));
        assert!(!source.contains("std::net"));
        assert!(!source.contains("TcpStream"));
    }
    let fixture = include_str!("fixture.rs");
    assert!(!fixture.contains("sim-runtime"));
    assert!(!fixture.contains("sim-private"));
    assert!(!fixture.contains("/home/"));
}
