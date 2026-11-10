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
}

#[test]
fn measure_why_reports_the_resident_query() {
    let measured = query::measure_why(4).unwrap();
    assert!(measured.within_ceiling);
    let rendered = crate::command::render(&measured.record);
    assert!(rendered.starts_with("#(world/query-latency-v1 "));
    assert!(rendered.contains("(command \"why\")"));
    assert!(rendered.contains("(records 100000)"));
    assert!(rendered.contains("(units 10000)"));
    assert!(rendered.contains("(warmups 20)"));
    assert!(rendered.contains("(samples 200)"));
    assert!(rendered.contains("(effects 4)"));
    assert!(rendered.contains("(subprocesses 0)"));
    assert!(rendered.contains("(resident true)"));
}

struct OracleRow {
    delta: &'static str,
    changed: &'static [&'static str],
    affected: &'static [&'static str],
    basis: u16,
    revision: u64,
}

fn oracle() -> [OracleRow; 6] {
    [
        OracleRow {
            delta: "D1",
            changed: &[],
            affected: &[],
            basis: 10_000,
            revision: 0,
        },
        OracleRow {
            delta: "D2",
            changed: &["change/private-implementation"],
            affected: &["conclusion/01", "conclusion/02"],
            basis: 9_500,
            revision: 0,
        },
        OracleRow {
            delta: "D3",
            changed: &["change/public-api"],
            affected: &[
                "conclusion/03",
                "conclusion/04",
                "conclusion/05",
                "conclusion/06",
                "conclusion/07",
                "conclusion/08",
                "conclusion/09",
                "conclusion/10",
            ],
            basis: 8_000,
            revision: 0,
        },
        OracleRow {
            delta: "D4",
            changed: &[
                "change/control-pins",
                "change/head-a",
                "change/head-b",
                "change/head-c",
            ],
            affected: &[
                "conclusion/11",
                "conclusion/12",
                "conclusion/13",
                "conclusion/14",
                "conclusion/15",
                "conclusion/16",
                "conclusion/17",
                "conclusion/18",
                "conclusion/19",
                "conclusion/20",
                "conclusion/21",
                "conclusion/22",
            ],
            basis: 7_000,
            revision: 0,
        },
        OracleRow {
            delta: "D5",
            changed: &["change/checker-alpha", "change/proof-code-alpha"],
            affected: &[
                "conclusion/23",
                "conclusion/24",
                "conclusion/25",
                "conclusion/26",
            ],
            basis: 9_000,
            revision: 0,
        },
        OracleRow {
            delta: "D6",
            changed: &[],
            affected: &[],
            basis: 10_000,
            revision: 1,
        },
    ]
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
        assert_eq!(row.journal_records, expected.changed.len());
        assert_eq!(row.explanations.len(), expected.affected.len());
    }
    let rendered = crate::command::render(&observation.rows[1].explanations[0]);
    assert!(rendered.contains("\"owner/owner/01\""));
    assert!(rendered.contains("\"conclusion/conclusion/01\""));
    assert!(rendered.contains("\"fact/change/private-implementation\""));
    let again = crate::assay::read_semantics().unwrap();
    assert_eq!(again.subject, batch.subject);
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
