use sim_codec_classfile::inspect_classfile;
use sim_kernel::CodecId;

const PROTOCOLS: &str = include_str!("../lambda-bootstrap-protocols.toml");
const MALFORMED: &str = include_str!("../fixtures/lambda-malformed.toml");
const FIXTURES: &str = include_str!("../fixtures/lambda-fixtures.toml");
const LAMBDA_CLASS: &[u8] = include_bytes!("../fixtures/javac/LambdaFixtures.class");

#[test]
fn lambda_protocol_scope_is_machine_readable_and_closed() {
    let scope: toml::Value = PROTOCOLS.parse().unwrap();
    assert_eq!(
        scope["owner"].as_str(),
        Some("java/lang/invoke/LambdaMetafactory")
    );
    assert_eq!(scope["protocol"].as_array().unwrap().len(), 2);
    assert_eq!(scope["flags"]["admitted_mask"].as_integer(), Some(7));
    let kinds = scope["reference_kinds"].as_table().unwrap();
    assert_eq!(kinds.len(), 5);
    assert!(kinds.values().all(toml::Value::is_integer));

    let malformed: toml::Value = MALFORMED.parse().unwrap();
    assert_eq!(malformed["case"].as_array().unwrap().len(), 8);

    let fixtures: toml::Value = FIXTURES.parse().unwrap();
    let sites = fixtures["site"].as_array().unwrap();
    assert_eq!(sites.len(), 8);
    let kinds = sites
        .iter()
        .map(|site| site["reference_kind"].as_integer().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(kinds, [5, 6, 7, 8, 9].into_iter().collect());
}

#[test]
fn frozen_javac_fixture_contains_all_lambda_shapes() {
    assert_eq!(&LAMBDA_CLASS[0..4], &[0xca, 0xfe, 0xba, 0xbe]);
    inspect_classfile(CodecId(139), LAMBDA_CLASS.to_vec(), 65_536).unwrap();
    assert!(LAMBDA_CLASS.iter().filter(|byte| **byte == 0xba).count() >= 8);
}

#[test]
fn required_owner_organs_and_shared_consumers_are_present() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let ledger = std::fs::read_to_string(root.join("reuse-ledger.toml")).unwrap();
    for anchor in ["#FunctionPlan", "#FunctionInstance", "#ClassDescriptor"] {
        assert!(ledger.contains(anchor), "missing reuse owner {anchor}");
    }

    let scope: toml::Value = PROTOCOLS.parse().unwrap();
    for consumer in ["registry_consumer", "verifier_consumer"] {
        let anchor = scope[consumer].as_str().unwrap();
        let (path, symbol) = anchor.split_once('#').unwrap();
        let source = std::fs::read_to_string(root.join("../..").join(path)).unwrap();
        assert!(source.contains(symbol), "missing {consumer} owner {anchor}");
    }
}
