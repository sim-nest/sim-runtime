// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// conformance: projection tests prove envelope-independent project, diff, and why.

use std::time::{Duration, Instant};

use sim_incremental_core::projection::{ConclusionId, FactId};
use sim_kernel::{CapabilityName, Datum, Dependency, Export, Lib, Symbol};

use super::*;
use crate::query::{self, nearest_rank_p95};

#[test]
fn bundled_product_projects_and_preserves_envelope_independence() {
    let product = WorldProduct::bundled().unwrap();
    let a = product
        .project(
            "world/public-api-v1",
            SOURCE_FACT,
            Datum::String("api-v1".to_owned()),
            Some(Datum::String("host/a attempt/1".to_owned())),
        )
        .unwrap();
    let b = product
        .project(
            "world/public-api-v1",
            SOURCE_FACT,
            Datum::String("api-v1".to_owned()),
            Some(Datum::String("host/b attempt/9".to_owned())),
        )
        .unwrap();
    assert_eq!(a.result.digest, b.result.digest);
    assert_eq!(a.result.affected.len(), 2);
    assert_eq!(product.effect_calls(), 0);
}

#[test]
fn disclosure_change_invalidates_only_its_consumer() {
    let product = WorldProduct::bundled().unwrap();
    let diff = product
        .diff(
            "no-v3/disclosure-policy-v1",
            DISCLOSURE_FACT,
            Datum::String("allowlist/a".to_owned()),
            Datum::String("allowlist/b".to_owned()),
        )
        .unwrap();
    let Datum::Node { fields, .. } = diff else {
        panic!("diff is not a record")
    };
    let affected = fields
        .iter()
        .find(|(name, _)| name.to_string() == "affected")
        .map(|(_, value)| value)
        .unwrap();
    assert_eq!(
        affected,
        &Datum::Vector(vec![Datum::String(DISCLOSURE_CONCLUSION.to_owned())])
    );
}

#[test]
fn why_is_exact_cached_and_has_no_effect_route() {
    let product = WorldProduct::bundled().unwrap();
    for _ in 0..20 {
        product.why(DISCLOSURE_CONCLUSION, DISCLOSURE_FACT).unwrap();
    }
    let mut samples = Vec::with_capacity(200);
    for _ in 0..200 {
        let started = Instant::now();
        let value = product.why(DISCLOSURE_CONCLUSION, DISCLOSURE_FACT).unwrap();
        samples.push(started.elapsed());
        assert!(matches!(value, Datum::Node { .. }));
    }
    samples.sort_unstable();
    let p95 = samples[189];
    assert!(p95 <= Duration::from_millis(200), "p95 was {p95:?}");
    assert_eq!(product.effect_calls(), 0);
}

#[test]
fn model_load_requires_provider_loader_and_artifact() {
    let missing = WorldCommandLib::for_model_load(ModelDomainLoad {
        provider: String::new(),
        loader: "loader/static-v1".to_owned(),
        artifact: "lib/world-command".to_owned(),
    });
    assert!(missing.is_err());
}

#[test]
fn command_is_a_loaded_world_entrypoint_without_capabilities() {
    let command = WorldCommandLib::new().unwrap();
    let manifest = command.manifest();
    assert!(manifest.capabilities.is_empty());
    assert!(manifest.requires.is_empty());
    assert!(manifest.exports.iter().any(|export| matches!(
        export,
        Export::Function { symbol, .. } if symbol.to_string() == "cli/main/world"
    )));
}

#[test]
fn outside_plan_subject_uses_the_shipped_command() {
    let owner = "outside/plan-harbor";
    for source in [include_str!("plan.rs"), include_str!("plan_subject.rs")] {
        assert!(!source.contains(owner));
    }
    let document = format!(
        "\
sim-world-plan-v1
fact fact/plan-harbor beacon
spec conclusion/plan-harbor {owner} evidence/current current fact/plan-harbor
"
    );
    let command = WorldCommandLib::new().expect("command");
    let planned =
        crate::command::dispatch_argv(&command, &["sim".to_owned(), "plan".to_owned(), document])
            .expect("plan");
    let rendered = crate::command::render(&planned);
    assert!(rendered.contains("conclusion/plan-harbor"));
    assert!(rendered.contains(owner));
}

#[test]
fn outside_owner_uses_the_shipped_command() {
    let owner = "outside/harbor-beacon";
    for source in [
        include_str!("command.rs"),
        include_str!("product.rs"),
        include_str!("fixture.rs"),
        include_str!("observe.rs"),
        include_str!("observe_rows.rs"),
        include_str!("seal.rs"),
        include_str!("plan.rs"),
    ] {
        assert!(
            !source.contains(owner),
            "{owner} is baked into the executor"
        );
    }
    let command = WorldCommandLib::declared(
        ModelDomainLoad {
            provider: "outside/caller-site".to_owned(),
            loader: "loader/outside".to_owned(),
            artifact: "lib/outside-world".to_owned(),
        },
        vec![Declaration {
            kind: "world/package-assembly-v1".to_owned(),
            fact: "fact/harbor-beacon".to_owned(),
            owner: owner.to_owned(),
            conclusion: "conclusion/harbor-beacon".to_owned(),
        }],
    )
    .expect("declared load");
    let projected = crate::command::dispatch_argv(
        &command,
        &[
            "sim".to_owned(),
            "project".to_owned(),
            "world/package-assembly-v1".to_owned(),
            "fact/harbor-beacon".to_owned(),
            "beacon-body".to_owned(),
        ],
    )
    .expect("project");
    assert!(crate::command::render(&projected).contains("beacon-body"));
    let explained = crate::command::dispatch_argv(
        &command,
        &[
            "sim".to_owned(),
            "why".to_owned(),
            "conclusion/harbor-beacon".to_owned(),
            "fact/harbor-beacon".to_owned(),
        ],
    )
    .expect("why");
    assert!(crate::command::render(&explained).contains("owner/outside/harbor-beacon"));
    let named = WorldCommandLib::new().expect("bundled");
    let gap = crate::command::dispatch_argv(
        &named,
        &["sim".to_owned(), "seal".to_owned(), "owning".to_owned()],
    )
    .expect("named seal");
    assert!(crate::command::render(&gap).contains("unknown-batch"));
}

#[test]
fn unknown_pair_is_unsupported_and_hostile_text_stays_data() {
    let product = WorldProduct::bundled().unwrap();
    let error = product
        .project(
            "no-such/kind",
            "no-such/fact",
            Datum::String("mutated".to_owned()),
            None,
        )
        .unwrap_err();
    assert!(matches!(error, WorldError::UnsupportedPair { .. }));
    let projected = product
        .project(
            "world/public-api-v1",
            SOURCE_FACT,
            Datum::String("#(mutate world)".to_owned()),
            None,
        )
        .unwrap();
    assert_eq!(product.effect_calls(), 0);
    assert!(crate::command::render(&projected.value).contains("\"#(mutate world)\""));
}

#[test]
fn read_only_record_renders_supplied_manifest_lists() {
    let record = query::read_only_record(
        0,
        &[CapabilityName::new("table.fs.read")],
        &[Dependency {
            id: Symbol::qualified("lib", "example"),
            minimum_version: None,
        }],
    );
    assert_eq!(
        crate::command::render(&record),
        "#(world/read-only-v1 (effects 0) (capabilities [\"table.fs.read\"]) (requires [\"lib/example\"]) (resident true))"
    );
}

#[test]
fn resident_snapshot_has_the_cached_query_denominator() {
    assert_eq!(nearest_rank_p95(200), 189);
    let snapshot = query::seal_resident_why().unwrap();
    assert_eq!(snapshot.records, 100_000);
    assert_eq!(snapshot.units, 10_000);
    assert_eq!(snapshot.closure.conclusions().len(), 10_000);
    let first = ConclusionId::new("conclusion/unit-000000").unwrap();
    let last = ConclusionId::new("conclusion/unit-009999").unwrap();
    let fact0 = FactId::new("fact/000000").unwrap();
    let fact10 = FactId::new("fact/000010").unwrap();
    let fact_last_start = FactId::new("fact/099990").unwrap();
    let fact_last = FactId::new("fact/099999").unwrap();
    assert!(snapshot.closure.depends_on(&first, &fact0));
    assert!(!snapshot.closure.depends_on(&first, &fact10));
    assert!(snapshot.closure.depends_on(&last, &fact_last_start));
    assert!(snapshot.closure.depends_on(&last, &fact_last));
    assert_eq!(snapshot.closure.fact_count(), 100_000);
    assert!(snapshot.closure.contains_fact(&fact_last));
    assert!(
        !snapshot
            .closure
            .contains_fact(&FactId::new("fact/100000").unwrap())
    );
    let explanation = snapshot.closure.explain(&first, &fact0).unwrap();
    assert_eq!(
        explanation.path,
        vec![
            "owner/sim-runtime/sim-lib-world".to_owned(),
            "conclusion/conclusion/unit-000000".to_owned(),
            "fact/fact/000000".to_owned(),
        ]
    );
    let product = WorldProduct::resident(snapshot.closure).unwrap();
    let (status, work) = product.status_work();
    assert_eq!(work.effects(), 0);
    assert_eq!(work.subprocesses(), 0);
    assert_eq!(work.reads(), 2);
    let rendered = crate::command::render(&status);
    assert!(rendered.starts_with("#(world/resident-status-v1 "));
    assert!(rendered.contains("(records 100000)"));
    assert!(rendered.contains("(units 10000)"));
    assert!(rendered.contains("(resident true)"));
}

#[test]
fn measure_why_reports_the_resident_query() {
    let measured = query::measure_why().unwrap();
    assert!(measured.within_ceiling);
    let rendered = crate::command::render(&measured.record);
    assert!(rendered.starts_with("#(world/query-latency-v1 "));
    assert!(rendered.contains("(command \"why\")"));
    assert!(rendered.contains("(records 100000)"));
    assert!(rendered.contains("(units 10000)"));
    assert!(rendered.contains("(warmups 20)"));
    assert!(rendered.contains("(samples 200)"));
    assert!(rendered.contains("(effects 0)"));
    assert!(rendered.contains("(raw-us ["));
    assert!(rendered.contains("(subprocesses 0)"));
    assert!(rendered.contains("(resident true)"));
}

#[test]
fn resident_cached_status_and_why_use_the_shipped_command() {
    let command = WorldCommandLib::new().expect("command");
    for verb in ["status", "why"] {
        let measured = crate::command::dispatch_argv(
            &command,
            &["sim".to_owned(), "measure".to_owned(), verb.to_owned()],
        )
        .unwrap_or_else(|error| panic!("{verb} dispatch: {error}"));
        assert_resident_latency(&measured, verb);
    }
}

fn assert_resident_latency(value: &Datum, command: &str) {
    let field = |name: &str| latency_field(value, name);
    let count = |name: &str| latency_count(field(name));
    assert_eq!(field("command"), &Datum::String(command.to_owned()));
    assert_eq!(count("records"), 100_000);
    assert_eq!(count("units"), 10_000);
    assert_eq!(count("warmups"), u64::try_from(query::WARMUPS).unwrap());
    assert_eq!(count("samples"), u64::try_from(query::SAMPLES).unwrap());
    assert_eq!(count("effects"), 0);
    assert_eq!(count("subprocesses"), 0);
    assert_eq!(field("resident"), &Datum::Bool(true));
    let Datum::Vector(raw) = field("raw-us") else {
        panic!("raw-us is not a vector");
    };
    assert_eq!(raw.len(), query::SAMPLES);
    let mut ordered: Vec<u64> = raw.iter().map(latency_count).collect();
    let rank = nearest_rank_p95(ordered.len());
    assert_eq!(rank, (ordered.len() * 95).div_ceil(100) - 1);
    ordered.sort_unstable();
    let p95_ms = ordered[rank] / 1_000;
    assert_eq!(count("p95-ms"), p95_ms);
    eprintln!(
        "{command} resident p95 {p95_ms} ms raw-us {}",
        ordered[rank]
    );
    assert!(query::within_cached_ceiling(ordered[rank]), "{command}");
}

fn latency_field<'a>(value: &'a Datum, name: &str) -> &'a Datum {
    let Datum::Node { tag, fields } = value else {
        panic!("latency record, got {value:?}");
    };
    assert_eq!(tag.as_qualified_str(), "world/query-latency-v1");
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, field)| field)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn latency_count(value: &Datum) -> u64 {
    let Datum::Number(number) = value else {
        panic!("count, got {value:?}");
    };
    number.canonical.parse().expect("count")
}

fn oracle() -> &'static [crate::assay_oracle::Expected] {
    crate::assay_oracle::expected()
}

fn names<T: std::fmt::Display>(values: &std::collections::BTreeSet<T>) -> Vec<String> {
    values.iter().map(ToString::to_string).collect()
}

fn owned(values: &[&str]) -> Vec<String> {
    values.iter().copied().map(str::to_owned).collect()
}

#[test]
fn assay_matches_the_frozen_oracle_and_measures_provider_reads() {
    let batch = crate::assay::read_semantics().unwrap();
    assert_eq!(batch.effects, 0);
    assert_eq!(batch.subprocesses, 0);
    assert_eq!(batch.requests, u64::try_from(batch.facts).unwrap() * 2 * 6);
    assert_eq!(batch.facts, 50);
    let observation = crate::assay::judge(&batch, None).unwrap();
    assert_eq!(observation.denominator, 40);
    assert_eq!(observation.subject.len(), 64);
    for (row, expected) in observation.rows.iter().zip(oracle()) {
        assert_eq!(row.delta, expected.delta);
        assert_eq!(names(&row.changed), owned(expected.changed));
        assert_eq!(names(&row.affected), owned(expected.affected));
        assert_eq!(row.unaffected_basis_points, expected.basis);
        assert_eq!(row.revision_delta, expected.revision);
        assert!(!row.carried_state);
        assert_eq!(row.planned_starts, expected.affected.len());
        assert_eq!(row.journal_records, 0);
        assert_eq!(row.explanations.len(), expected.affected.len());
    }
    let rendered = crate::command::render(&observation.rows[1].explanations[0]);
    assert!(rendered.contains("\"owner/owner/01\""));
    assert!(rendered.contains("\"conclusion/conclusion/01\""));
    assert!(rendered.contains("\"fact/change/private-implementation\""));
    let again = crate::assay::read_semantics().unwrap();
    assert_eq!(again.subject, batch.subject);
    let rows = crate::assay::subject_rows().unwrap().join("\n");
    assert!(rows.contains("edited private implementation"));
    assert!(rows.contains("edited public api"));
    assert!(rows.contains("postimage D1 stable/40 baseline stable/40 "));
    assert!(rows.contains("postimage D6 present/order alpha\nbeta"));
    assert!(rows.contains("graph owner/01 conclusion/01 stable/01 change/private-implementation"));
    assert!(rows.contains("struct WorldConfigShape"));
    assert!(rows.contains("fn read_semantics"));
    assert!(rows.contains("pub(super) fn execute"));
    assert!(rows.contains("fn ensure_mutants_diverge"));
    assert!(rows.contains("fn sealed_ids"));
    assert!(rows.contains("pub struct OwnerProjectionGraph"));
    assert!(rows.contains("pub fn execute_proof_leaf"));
    assert!(rows.contains("pub struct ExecutionJournal"));
    assert!(rows.contains("sim-roadmap-execution-record"));
    assert!(rows.contains("name = \"sim-lib-journal\""));
    assert!(rows.contains("pub(crate) struct LawCursor"));
    assert!(rows.contains("pub enum ExecutionRecord"));
    assert!(rows.contains("pub(crate) fn contains_secret"));
    assert!(rows.contains("pub(crate) fn apply_changes"));
    assert!(rows.contains("sim-world-proof-view-v1"));
    assert!(rows.contains("fn plan_record"));
    assert!(rows.contains("fn explanation_value"));
    assert!(rows.contains("fn count_datum"));
    assert!(rows.contains("name = \"sim-lib-world\""));
    assert!(rows.contains("projection/trusted-native-v1"));
    assert!(rows.contains("pure-check proof-code-alpha binds the projected bytes"));
    assert!(rows.contains("Independent expected sets"));
    assert!(rows.contains(&format!(
        "provider {} {}",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )));
    assert!(rows.contains("checker sim-incremental-core/projection/semantic-text"));
    assert!(rows.contains("provider-kind world/semantic-text-v1"));
    assert!(rows.contains("canonical_nan true"));
    assert!(rows.contains("canonical_collections true"));
    assert!(rows.contains("fresh_instance true"));
    assert!(rows.contains("requires_confinement false"));
    assert!(rows.contains("max_inputs 1"));
    assert!(rows.contains("max_output_bytes 65536"));
    assert!(rows.contains("max_fuel 1000000"));
    assert!(rows.contains("max_memory_bytes 4194304"));
    assert!(rows.contains("imports none"));
}

#[test]
fn assay_mutants_miss_the_frozen_oracle() {
    let batch = crate::assay::read_semantics().unwrap();
    let sound = crate::assay::judge(&batch, None).unwrap();
    assert!(oracle_match(&sound));
    for declaration in 1..=40 {
        let mutant = crate::assay::judge(
            &batch,
            Some(crate::assay::GraphMutation::Declaration(declaration)),
        )
        .unwrap();
        assert!(
            !oracle_match(&mutant),
            "declaration mutant {declaration} survived"
        );
    }
    for declaration in 1..=40 {
        let edges = 1 + usize::from(declaration <= 26);
        for edge in 0..edges {
            let mutant = crate::assay::judge(
                &batch,
                Some(crate::assay::GraphMutation::Edge(declaration, edge)),
            )
            .unwrap();
            assert!(
                !oracle_match(&mutant),
                "edge mutant {declaration}:{edge} survived"
            );
        }
    }
    assert!(
        crate::assay::refuse_undeclared_read(
            "conclusion/40",
            &["stable/40", "change/private-implementation"]
        )
        .is_err()
    );
    assert!(
        crate::assay::refuse_undeclared_read(
            "conclusion/01",
            &["stable/01", "change/private-implementation"]
        )
        .is_ok()
    );
}

fn oracle_match(observation: &crate::assay::AssayObservation) -> bool {
    observation.denominator == 40
        && observation.rows.len() == oracle().len()
        && observation
            .rows
            .iter()
            .zip(oracle())
            .all(|(row, expected)| {
                row.delta == expected.delta
                    && names(&row.changed) == owned(expected.changed)
                    && names(&row.affected) == owned(expected.affected)
                    && row.revision_delta == expected.revision
                    && !row.carried_state
            })
}
