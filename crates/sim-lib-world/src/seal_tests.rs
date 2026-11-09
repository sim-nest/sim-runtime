// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use sim_kernel::{Datum, Symbol};

use crate::command::render;
use crate::query::{self, SAMPLES, WARMUPS};
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

fn has_field(fields: &[(Symbol, Datum)], name: &str) -> bool {
    fields
        .iter()
        .any(|(symbol, _)| symbol.as_qualified_str() == name)
}

fn checksums(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("checksum = \"") else {
            continue;
        };
        let Some(end) = rest.find('"') else {
            continue;
        };
        found.insert(rest[..end].to_owned());
    }
    found
}

fn workspace_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn locks(root: &Path, out: &mut Vec<std::path::PathBuf>) {
    let entries = fs::read_dir(root).expect("read workspace");
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name == "target" || name == ".git" || name == ".sim" {
                continue;
            }
            locks(&path, out);
        } else if name == "Cargo.lock" {
            out.push(path);
        }
    }
}

#[test]
fn owning_seal_matches_live_locks_and_stays_stable() {
    let first = seal::open("owning", None).expect("seal");
    let second = seal::open("owning", None).expect("repeat");
    assert_eq!(render(&first), render(&second));
    let fields = node(&first, "world/source-seal-v1");
    assert_eq!(text(field(fields, "admission")), "refuse");
    assert_eq!(field(fields, "compiler-opened"), &Datum::Bool(false));
    assert_eq!(field(fields, "effects"), &query::count_datum(0));
    assert_eq!(field(fields, "subprocesses"), &query::count_datum(0));
    assert_eq!(field(fields, "resident"), &Datum::Bool(true));
    let digest = text(field(fields, "digest")).to_owned();
    assert_eq!(digest.len(), 64);

    let universe = seal::universe("owning", None).expect("universe");
    let rows = node(&universe, "world/source-universe-v1");
    assert_eq!(text(field(rows, "digest")), digest);
    assert_eq!(field(rows, "compiler-opened"), &Datum::Bool(false));
    let rendered = render(&universe);
    let root = workspace_root();
    let mut lock_paths = Vec::new();
    locks(&root, &mut lock_paths);
    assert!(lock_paths.len() >= 2, "nested Cargo locks are present");
    assert_eq!(number(field(fields, "domains")), lock_paths.len() as u64);
    let mut expected = BTreeSet::new();
    for path in &lock_paths {
        let text = fs::read_to_string(path).expect("read lock");
        expected.extend(checksums(&text));
        let relative = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
        if relative != "Cargo.lock" {
            assert!(
                rendered.contains(relative.trim_end_matches("/Cargo.lock")),
                "missing nested domain {relative}"
            );
        }
    }
    assert!(!expected.is_empty());
    for checksum in &expected {
        assert!(rendered.contains(checksum), "missing checksum {checksum}");
    }

    let generated = vector(field(rows, "generated-rows"));
    let source = root.join("crates/sim-lib-lang-jvm/src/superinstructions_generated.rs");
    let length = fs::metadata(&source).expect("generated file").len();
    let row = generated
        .iter()
        .map(|value| node(value, "world/generated-input-v1"))
        .find(|row| text(field(row, "path")).ends_with("superinstructions_generated.rs"))
        .expect("generated input");
    assert_eq!(number(field(row, "bytes")), length);
    let features = vector(field(rows, "feature-rows"));
    assert!(features.iter().any(|value| {
        text(value).contains("native-export") && text(value).contains("dep:sim-codec-binary")
    }));
    assert!(features.iter().any(|value| {
        text(value).contains("generated-coverage")
            && text(value).contains("dep:sim-lib-lang-genconf")
    }));
    assert!(!rendered.contains("lock-identity"));
    assert!(!rendered.contains("path+workspace"));
    assert!(!rendered.contains("/home/"));

    let members = vector(field(rows, "member-rows"));
    let world = members
        .iter()
        .map(|value| node(value, "world/source-member-v1"))
        .find(|row| text(field(row, "name")) == "sim-lib-world")
        .expect("sim-lib-world");
    assert_eq!(text(field(world, "status")), "verified");
    assert_eq!(text(field(world, "origin")), "path");
    assert_eq!(text(field(world, "source")), "crates/sim-lib-world");
    assert_eq!(text(field(world, "checksum-kind")), "path-content");
    assert_eq!(field(world, "closure"), &Datum::Bool(true));
    assert_eq!(
        number(field(world, "files")),
        package_files(&root.join("crates/sim-lib-world"))
    );
    assert!(
        vector(field(world, "dependencies"))
            .iter()
            .any(|value| text(value).starts_with("sim-kernel"))
    );
    assert_ne!(
        text(field(world, "content")),
        text(field(world, "checksum"))
    );
    let mut mismatched = 0u64;
    let mut unreached = 0u64;
    let mut outside = 0u64;
    for value in members {
        let row = node(value, "world/source-member-v1");
        if text(field(row, "origin")) == "path" {
            let source = text(field(row, "source"));
            if source == "missing-path" {
                assert_eq!(text(field(row, "status")), "present-unverified");
                assert_eq!(text(field(row, "reason")), "missing-checksum");
                assert!(!has_field(row, "checksum-kind"));
                outside += 1;
                continue;
            }
            assert_eq!(text(field(row, "checksum-kind")), "path-content");
            assert!(root.join(source).is_dir(), "{source}");
            assert!(!source.starts_with('/'), "{source}");
            let manifest = if source == "." {
                root.join("Cargo.toml")
            } else {
                root.join(source).join("Cargo.toml")
            };
            let declared = manifest_version(&manifest);
            if declared == text(field(row, "version")) {
                assert_eq!(text(field(row, "status")), "verified");
                assert!(!has_field(row, "reason"));
            } else {
                mismatched += 1;
                assert_eq!(text(field(row, "status")), "present-unverified");
                assert_eq!(text(field(row, "reason")), "version-mismatch");
                assert_eq!(
                    text(field(row, "content")),
                    format!(
                        "{} {} {} {} {}",
                        text(field(row, "domain")),
                        text(field(row, "origin")),
                        text(field(row, "name")),
                        declared,
                        source
                    )
                );
                assert_ne!(text(field(row, "content")), text(field(row, "checksum")));
                assert_eq!(
                    number(field(row, "files")),
                    package_files(&root.join(source))
                );
            }
        }
        if text(field(row, "origin")) == "registry" || text(field(row, "origin")) == "git" {
            let status = text(field(row, "status"));
            if status == "verified" {
                assert!(!has_field(row, "reason"));
                assert_ne!(text(field(row, "content")), text(field(row, "checksum")));
            } else {
                unreached += 1;
                assert_eq!(status, "present-unverified");
                let reason = text(field(row, "reason"));
                assert!(
                    reason == "source-not-reached" || reason == "corrupt-checksum",
                    "{reason}"
                );
                assert_ne!(text(field(row, "content")), text(field(row, "checksum")));
            }
        }
        if has_field(row, "checksum-kind") {
            assert_ne!(text(field(row, "checksum-kind")), "lock-identity");
        }
    }
    assert!(mismatched > 0, "a nested lock disagrees with its manifest");
    assert_eq!(
        number(field(fields, "present-unverified")),
        mismatched + unreached + outside
    );
    assert_eq!(
        number(field(fields, "verified")) + mismatched + unreached + outside,
        number(field(fields, "members"))
    );
    let first_gap = members
        .iter()
        .map(|value| node(value, "world/source-member-v1"))
        .find(|row| text(field(row, "status")) != "verified")
        .expect("unverified member");
    let shown = text(field(first_gap, "reason"));
    let expected_reason = match shown {
        "corrupt-checksum" => "corrupt-input",
        "missing-checksum" => "missing-input",
        other => other,
    };
    assert_eq!(text(field(fields, "reason")), expected_reason);
    assert_eq!(
        text(field(fields, "coordinate")),
        format!(
            "{} {} {}",
            text(field(first_gap, "domain")),
            text(field(first_gap, "name")),
            text(field(first_gap, "version"))
        )
    );
    let registry = members
        .iter()
        .map(|value| node(value, "world/source-member-v1"))
        .find(|row| text(field(row, "checksum-kind")) == "registry")
        .expect("registry member");
    assert!(expected.contains(text(field(registry, "checksum"))));
    assert_ne!(
        text(field(registry, "content")),
        text(field(registry, "checksum"))
    );
}

#[test]
fn defective_inputs_refuse_before_compile_and_keep_statuses() {
    let cases = [
        (
            "missing",
            "missing-input",
            "present-unverified",
            "missing-checksum",
        ),
        (
            "corrupt",
            "corrupt-input",
            "present-unverified",
            "corrupt-checksum",
        ),
        ("stub", "stub-only", "stub-only", "metadata-only"),
        ("absent", "absent-input", "absent", ""),
        (
            "nested",
            "missing-input",
            "present-unverified",
            "missing-checksum",
        ),
        (
            "git-missing",
            "missing-input",
            "present-unverified",
            "missing-checksum",
        ),
        (
            "version-mismatch",
            "version-mismatch",
            "present-unverified",
            "version-mismatch",
        ),
    ];
    for (batch, reason, status, detail) in cases {
        let value = seal::open(batch, None).expect(batch);
        let fields = node(&value, "world/source-seal-v1");
        assert_eq!(text(field(fields, "admission")), "refuse", "{batch}");
        assert_eq!(text(field(fields, "reason")), reason, "{batch}");
        assert_eq!(
            field(fields, "compiler-opened"),
            &Datum::Bool(false),
            "{batch}"
        );
        assert_eq!(field(fields, "effects"), &query::count_datum(0));
        assert_eq!(field(fields, "subprocesses"), &query::count_datum(0));
        let universe = seal::universe(batch, None).expect(batch);
        let rows = node(&universe, "world/source-universe-v1");
        if batch == "nested" {
            assert_eq!(number(field(rows, "domains")), 2);
            let ids: Vec<_> = vector(field(rows, "domain-rows"))
                .iter()
                .map(|value| text(field(node(value, "world/resolver-domain-v1"), "id")).to_owned())
                .collect();
            assert!(ids.iter().any(|id| id.ends_with("/vendor/other")));
            assert!(ids.iter().any(|id| id == "resolver/parent"));
            assert_ne!(ids[0], ids[1]);
            let nested = vector(field(rows, "domain-rows"))
                .iter()
                .map(|value| node(value, "world/resolver-domain-v1"))
                .find(|row| text(field(row, "id")).ends_with("/vendor/other"))
                .expect("nested domain");
            assert_eq!(text(field(nested, "role")), "enumerated");
        }
        let members = vector(field(rows, "member-rows"));
        assert!(
            members.iter().any(|value| {
                let row = node(value, "world/source-member-v1");
                text(field(row, "status")) == status
                    && (detail.is_empty()
                        || !has_field(row, "reason")
                        || text(field(row, "reason")) == detail)
            }),
            "{batch} status {status}"
        );
        if batch == "version-mismatch" {
            let stale = members
                .iter()
                .map(|value| node(value, "world/source-member-v1"))
                .find(|row| text(field(row, "name")) == "stale-crate")
                .expect("stale member");
            assert_eq!(text(field(stale, "version")), "0.1.0");
            assert_eq!(
                text(field(stale, "content")),
                "resolver/demo path stale-crate 0.2.0 crates/stale"
            );
            assert_eq!(text(field(stale, "checksum")), "def");
            assert_eq!(text(field(stale, "checksum-kind")), "path-content");
            let fresh = members
                .iter()
                .map(|value| node(value, "world/source-member-v1"))
                .find(|row| text(field(row, "name")) == "demo-crate")
                .expect("verified member");
            assert_eq!(text(field(fresh, "status")), "verified");
            assert_eq!(
                text(field(fields, "coordinate")),
                "resolver/demo stale-crate 0.1.0"
            );
        }
    }
}

#[test]
fn aliases_canonicalize_without_merging_the_nested_domain() {
    let value = seal::universe("alias", None).expect("alias");
    let fields = node(&value, "world/source-universe-v1");
    assert_eq!(text(field(fields, "admission")), "admit");
    assert_eq!(field(fields, "compiler-opened"), &Datum::Bool(false));
    assert_eq!(number(field(fields, "domains")), 2);
    let domains = vector(field(fields, "domain-rows"));
    let parent = domains
        .iter()
        .map(|value| node(value, "world/resolver-domain-v1"))
        .find(|row| text(field(row, "id")) == "/work/sim-runtime")
        .expect("parent domain");
    let aliases = vector(field(parent, "aliases"));
    assert_eq!(aliases.len(), 3);
    let nested = domains
        .iter()
        .map(|value| node(value, "world/resolver-domain-v1"))
        .find(|row| text(field(row, "id")) == "/work/sim-runtime/nested")
        .expect("nested domain");
    assert_ne!(text(field(parent, "id")), text(field(nested, "id")));
    assert_eq!(vector(field(nested, "aliases")).len(), 1);
}

#[test]
fn wrong_target_keeps_the_digest_and_drops_the_other_status() {
    let admitted = seal::open("owning", None).expect("owning");
    let fields = node(&admitted, "world/source-seal-v1");
    let sealed = text(field(fields, "sealed-target"));
    let other = if sealed == "wasm32-unknown-unknown" {
        "x86_64-unknown-linux-gnu"
    } else {
        "wasm32-unknown-unknown"
    };
    let refused = seal::open("owning", Some(other)).expect("wrong target");
    let refused_fields = node(&refused, "world/source-seal-v1");
    assert_eq!(text(field(refused_fields, "admission")), "refuse");
    assert_eq!(text(field(refused_fields, "reason")), "wrong-target");
    assert_eq!(
        text(field(refused_fields, "digest")),
        text(field(fields, "digest"))
    );
    assert_eq!(
        field(refused_fields, "compiler-opened"),
        &Datum::Bool(false)
    );
    assert_eq!(number(field(refused_fields, "verified")), 0);
    assert_eq!(
        number(field(refused_fields, "absent")),
        number(field(fields, "members"))
    );
    let universe = seal::universe("owning", Some(other)).expect("universe");
    let row = node(
        &vector(field(
            node(&universe, "world/source-universe-v1"),
            "member-rows",
        ))[0],
        "world/source-member-v1",
    );
    assert_eq!(text(field(row, "status")), "absent");
    assert_eq!(text(field(row, "target")), other);
    assert!(!has_field(row, "checksum"));
    assert!(!has_field(row, "content"));
}

#[test]
fn one_changed_checksum_invalidates_that_input() {
    let owning = seal::open("owning", None).expect("owning");
    let changed = seal::open("changed", None).expect("changed");
    let owning_fields = node(&owning, "world/source-seal-v1");
    let changed_fields = node(&changed, "world/source-seal-v1");
    assert_eq!(text(field(owning_fields, "admission")), "refuse");
    assert_eq!(text(field(changed_fields, "admission")), "refuse");
    assert_ne!(
        text(field(owning_fields, "digest")),
        text(field(changed_fields, "digest"))
    );
    let diff = seal::diff("owning", "changed", None).expect("diff");
    let fields = node(&diff, "world/source-seal-diff-v1");
    assert_eq!(field(fields, "changed"), &Datum::Bool(true));
    assert_eq!(field(fields, "compiler-opened"), &Datum::Bool(false));
    assert_eq!(field(fields, "effects"), &query::count_datum(0));
    assert_eq!(field(fields, "subprocesses"), &query::count_datum(0));
    let inputs = vector(field(fields, "inputs"));
    assert_eq!(inputs.len(), 1);
    let change = node(&inputs[0], "world/changed-input-v1");
    assert_eq!(text(field(change, "field")), "checksum");
    let name = text(field(change, "name")).to_owned();
    let version = text(field(change, "version")).to_owned();
    let before = member_checksum("owning", &name, &version);
    let after = member_checksum("changed", &name, &version);
    assert_ne!(before, after);
    let same = seal::diff("owning", "owning", None).expect("same");
    let same_fields = node(&same, "world/source-seal-diff-v1");
    assert_eq!(field(same_fields, "changed"), &Datum::Bool(false));
    assert!(vector(field(same_fields, "inputs")).is_empty());
}

#[test]
fn diff_names_the_changed_resolver_field() {
    let sealed = seal::open("precise", None).expect("precise");
    let fields = node(&sealed, "world/source-seal-v1");
    assert_eq!(text(field(fields, "admission")), "refuse");
    assert_eq!(text(field(fields, "reason")), "source-not-reached");
    assert_eq!(field(fields, "compiler-opened"), &Datum::Bool(false));
    let cases = [
        ("precise-toolchain", "toolchain", "toolchain"),
        ("precise-feature", "feature", "feature"),
        ("precise-generated", "generated", "checksum"),
        ("precise-edge", "registry", "dependencies"),
        ("precise-closure", "registry", "closure"),
        ("precise-role", "domain", "role"),
    ];
    for (batch, origin, field_name) in cases {
        let diff = seal::diff("precise", batch, None).expect(batch);
        let fields = node(&diff, "world/source-seal-diff-v1");
        assert_eq!(field(fields, "changed"), &Datum::Bool(true), "{batch}");
        assert_eq!(field(fields, "compiler-opened"), &Datum::Bool(false));
        let inputs = vector(field(fields, "inputs"));
        assert_eq!(inputs.len(), 1, "{batch}");
        let change = node(&inputs[0], "world/changed-input-v1");
        assert_eq!(text(field(change, "field")), field_name, "{batch}");
        assert_eq!(text(field(change, "origin")), origin, "{batch}");
    }
}

fn manifest_version(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap_or_else(|error| panic!("read {path:?}: {error}"));
    let mut section = String::new();
    let mut version = String::new();
    for line in text.lines() {
        let trim = line.trim();
        if trim.starts_with('[') && trim.ends_with(']') {
            section = trim[1..trim.len() - 1].to_owned();
            continue;
        }
        if section != "package" {
            continue;
        }
        let Some(rest) = trim.strip_prefix("version") else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('"') else {
            continue;
        };
        let Some(end) = rest.find('"') else {
            continue;
        };
        version = rest[..end].to_owned();
    }
    assert!(!version.is_empty(), "package version in {}", path.display());
    version
}

fn package_files(root: &Path) -> u64 {
    let mut count = 0u64;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in fs::read_dir(&dir).expect("read package") {
            let entry = entry.expect("entry");
            let file_type = entry.file_type().expect("file type");
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if name == "target" || name == ".git" || name == ".sim" {
                    continue;
                }
                stack.push(entry.path());
            } else if file_type.is_file() {
                count += 1;
            }
        }
    }
    count
}

fn member_checksum(batch: &str, name: &str, version: &str) -> String {
    let universe = seal::universe(batch, None).expect(batch);
    let rows = node(&universe, "world/source-universe-v1");
    let row = vector(field(rows, "member-rows"))
        .iter()
        .map(|value| node(value, "world/source-member-v1"))
        .find(|row| text(field(row, "name")) == name && text(field(row, "version")) == version)
        .unwrap_or_else(|| panic!("missing {name} {version}"));
    text(field(row, "checksum")).to_owned()
}

#[test]
fn unknown_batch_is_a_gap_and_measure_stays_inside_the_ceiling() {
    let gap = seal::open("no-such-batch", None).expect("gap");
    let fields = node(&gap, "world/source-gap-v1");
    assert_eq!(text(field(fields, "reason")), "unknown-batch");
    assert_eq!(field(fields, "compiler-opened"), &Datum::Bool(false));
    assert_eq!(field(fields, "effects"), &query::count_datum(0));
    let measured = seal::measure(0).expect("measure");
    let record = node(&measured, "world/query-latency-v1");
    assert_eq!(text(field(record, "command")), "seal");
    assert_eq!(
        field(record, "warmups"),
        &query::count_datum(u64::try_from(WARMUPS).unwrap())
    );
    assert_eq!(
        field(record, "samples"),
        &query::count_datum(u64::try_from(SAMPLES).unwrap())
    );
    assert_eq!(field(record, "effects"), &query::count_datum(0));
    assert_eq!(field(record, "subprocesses"), &query::count_datum(0));
    assert_eq!(field(record, "resident"), &Datum::Bool(true));
    assert!(number(field(record, "p95-ms")) <= 200);
    assert!(number(field(record, "records")) > 0);
    let sources = [
        ("seal.rs", include_str!("seal.rs")),
        ("seal/batch.rs", include_str!("seal/batch.rs")),
        ("seal/emit.rs", include_str!("seal/emit.rs")),
        ("seal/parse.rs", include_str!("seal/parse.rs")),
        ("seal/marks.rs", include_str!("seal/marks.rs")),
    ];
    for (name, source) in sources {
        assert!(!source.contains("std::process"), "{name}");
        assert!(!source.contains("std::fs"), "{name}");
        assert!(!source.contains("std::net"), "{name}");
    }
}
