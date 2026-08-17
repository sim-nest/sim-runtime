//! Structural guard for the BYTECODE_SPEED_4 landed-source ownership ledger.

use std::{collections::BTreeMap, fs, path::Path};

use toml::Value;

#[test]
fn every_hot_path_anchor_owner_and_fallback_resolves() {
    let source_root = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .expect("JVM source directory must resolve");
    let crate_root = source_root.parent().unwrap();
    let repo_root = crate_root.ancestors().nth(2).unwrap();
    let ledger: Value = fs::read_to_string(crate_root.join("hot-path-ownership.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        ledger["schema"].as_str(),
        Some("sim.jvm-hot-path-ownership/v1")
    );

    let paths = ledger["path"].as_array().unwrap();
    let mut stages = BTreeMap::new();
    for row in paths {
        let stage = required(row, "stage");
        assert!(
            stages.insert(stage, row).is_none(),
            "duplicate stage {stage}"
        );
        assert!(!required(row, "owner").is_empty());
        assert!(!required(row, "fallback").is_empty());
        if required(row, "repository") == "sim-runtime" {
            assert_anchor(repo_root, required(row, "anchor"), required(row, "symbol"));
            if let Some(anchor) = row.get("supporting_anchor") {
                assert_anchor(
                    repo_root,
                    anchor.as_str().unwrap(),
                    required(row, "supporting_symbol"),
                );
            }
        }
    }

    let optimizations = ledger["optimization"].as_array().unwrap();
    assert_eq!(optimizations.len(), 8, "all optimization phases .04-.11");
    for row in optimizations {
        let phase = required(row, "phase");
        assert!(phase.starts_with("BYTECODESPEED4."));
        assert!(!required(row, "proposal").is_empty());
        assert!(
            stages.contains_key(required(row, "owner_stage")),
            "{phase} owner"
        );
        assert!(
            stages.contains_key(required(row, "fallback_stage")),
            "{phase} fallback"
        );
    }

    let contradictions = ledger["contradiction"].as_array().unwrap();
    assert!(!contradictions.is_empty());
    for row in contradictions {
        assert!(!required(row, "predecessor_assumption").is_empty());
        assert!(!required(row, "landed_source").is_empty());
        assert!(!required(row, "disposition").is_empty());
    }
}

fn required<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key).and_then(Value::as_str).unwrap()
}

fn assert_anchor(root: &Path, path: &str, symbol: &str) {
    let source = fs::read_to_string(root.join(path))
        .unwrap_or_else(|error| panic!("{path} does not resolve: {error}"));
    assert!(source.contains(symbol), "{path} does not contain {symbol}");
}
