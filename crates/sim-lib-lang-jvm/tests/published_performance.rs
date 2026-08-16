use std::{fs, path::Path};

#[test]
fn published_coverage_is_complete_and_traceable_to_raw_samples() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let coverage: toml::Value = fs::read_to_string(root.join("performance-coverage.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        coverage["schema"].as_str(),
        Some("sim.jvm-performance-coverage/v1")
    );
    assert_eq!(sim_lib_lang_jvm::VERIFIER_COVERAGE.opcode_rows, 256);
    let generated = include_str!("../src/superinstructions_generated.rs");
    assert!(generated.contains("pub const FUSED_DEFINITIONS"));
    assert!(generated.matches("FusedDefinition { handler:").count() > 0);

    let reports = coverage["benchmark"].as_array().unwrap();
    assert_eq!(reports.len(), 2);
    for report in reports {
        assert_eq!(report["owner_repository"].as_str(), Some("sim-tooling"));
        assert_eq!(report["samples_per_arm"].as_integer(), Some(20));
        assert_eq!(report["outcome"].as_str(), Some("inconclusive"));
        assert!(report["raw_artifact"].as_str().unwrap().ends_with(".json"));
        assert!(
            report["content_key"]
                .as_str()
                .unwrap()
                .starts_with("sha256:")
        );
    }
}

#[test]
fn publication_names_fidelity_and_ownership_faces() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let coverage = fs::read_to_string(root.join("performance-coverage.toml")).unwrap();
    let guide = fs::read_to_string(root.join("docs/jvm-bytecode-performance.md")).unwrap();
    for required in [
        "ExecutionPermit",
        "PreparedMicroOp",
        "FUSED_DEFINITIONS",
        "jvm-ownership.toml",
    ] {
        assert!(
            coverage.contains(required),
            "missing coverage anchor {required}"
        );
    }
    assert!(guide.contains("inconclusive"));
    assert!(guide.contains("raw samples"));
}
