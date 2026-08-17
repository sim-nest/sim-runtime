// conformance: JVM verification stays bounded to immutable admitted evidence.

use std::{collections::BTreeSet, fs, path::Path};

use toml::Value;

fn scope() -> Value {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/verifier-scope.toml"))
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn normative_clause_families_and_two_sided_fixtures_are_data() {
    let scope = scope();
    let versions = &scope["classfile_versions"];
    assert_eq!(versions["minimum_major"].as_integer(), Some(45));
    assert_eq!(versions["maximum_major"].as_integer(), Some(69));
    assert_eq!(versions["legacy_45_maximum_minor"].as_integer(), Some(3));
    assert_eq!(versions["preview_minor"].as_integer(), Some(65_535));

    let clauses = scope["clause_family"].as_array().unwrap();
    let expected: BTreeSet<_> = (1..=10).map(|n| format!("4.10.1.{n}")).collect();
    let actual: BTreeSet<_> = clauses
        .iter()
        .map(|clause| clause["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(clauses.len(), actual.len(), "duplicate clause family");
    for clause in clauses {
        assert!(!clause["positive"].as_str().unwrap().is_empty());
        assert!(!clause["negative"].as_str().unwrap().is_empty());
    }
}

#[test]
fn verifier_failure_boundary_is_exhaustive_and_disjoint() {
    let scope = scope();
    let rows = scope["failure_boundary"].as_array().unwrap();
    let expected: BTreeSet<_> = [
        ("MalformedCodecInput", "codec"),
        ("MissingVerificationDependency", "dependency"),
        ("VerificationResourceExhausted", "resource"),
        ("StaleVerificationProof", "stale-proof"),
        ("InternalRuleCoverage", "internal-coverage"),
        ("VerificationRejected", "java-verify-error"),
    ]
    .into_iter()
    .collect();
    let actual: BTreeSet<_> = rows
        .iter()
        .map(|row| {
            (
                row["condition"].as_str().unwrap(),
                row["home"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(rows.len(), actual.len(), "a condition appears in two rows");
    let verify_error = rows
        .iter()
        .find(|row| row["home"].as_str() == Some("java-verify-error"))
        .unwrap();
    assert_eq!(
        verify_error["java_class"].as_str(),
        Some("java/lang/VerifyError")
    );
}

#[test]
fn verifier_provider_is_the_only_integration_seam() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let contract = scope();
    assert_eq!(
        contract["integration"]["provider_trait"].as_str(),
        Some("VerifierProvider")
    );
    let entry = fs::read_to_string(root.join("src/entry.rs")).unwrap();
    assert_eq!(entry.matches("pub trait VerifierProvider").count(), 1);
    assert_eq!(entry.matches("provider.verify(").count(), 1);

    for source in fs::read_dir(root.join("src")).unwrap() {
        let source = source.unwrap().path();
        if source
            .extension()
            .is_some_and(|extension| extension == "rs")
            && source.file_name().unwrap() != "entry.rs"
        {
            let text = fs::read_to_string(&source).unwrap();
            assert!(
                !text.contains("provider.verify("),
                "second verifier integration point in {}",
                source.display()
            );
        }
    }
}
