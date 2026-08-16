use sim_codec_classfile::inspect_classfile;
use sim_kernel::CodecId;
use sim_lib_lang_jvm::{
    LambdaBootstrapError, ResolvedBootstrapArgument, decode_lambda_bootstrap,
    executor_admitted_lambda_protocols, verifier_admitted_lambda_protocols,
};

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
fn verifier_and_executor_admission_are_equal_by_construction() {
    assert_eq!(
        verifier_admitted_lambda_protocols(),
        executor_admitted_lambda_protocols()
    );
    let scope: toml::Value = PROTOCOLS.parse().unwrap();
    assert_eq!(
        executor_admitted_lambda_protocols().len(),
        scope["protocol"].as_array().unwrap().len()
    );
}

#[test]
fn malformed_constant_stage_precedes_linkage_and_allocation() {
    let protocol = &executor_admitted_lambda_protocols()[1];
    let valid = [
        ResolvedBootstrapArgument::MethodType("()Ljava/lang/Runnable;".into()),
        ResolvedBootstrapArgument::MethodHandle { reference_kind: 6 },
        ResolvedBootstrapArgument::MethodType("()Ljava/lang/Runnable;".into()),
        ResolvedBootstrapArgument::Integer(7),
        ResolvedBootstrapArgument::Integer(1),
        ResolvedBootstrapArgument::Class("java/io/Serializable".into()),
        ResolvedBootstrapArgument::Integer(1),
        ResolvedBootstrapArgument::MethodType("()V".into()),
    ];
    let plan = decode_lambda_bootstrap(protocol.owner, protocol.name, protocol.descriptor, &valid)
        .unwrap();
    assert!(plan.serializable);
    assert_eq!(plan.marker_interfaces, ["java/io/Serializable"]);
    assert_eq!(plan.bridges, ["()V"]);

    let mut truncated = valid.to_vec();
    truncated.pop();
    assert!(matches!(
        decode_lambda_bootstrap(
            protocol.owner,
            protocol.name,
            protocol.descriptor,
            &truncated
        ),
        Err(LambdaBootstrapError::MalformedPayload(_))
    ));
    let mut field_handle = valid.to_vec();
    field_handle[1] = ResolvedBootstrapArgument::MethodHandle { reference_kind: 1 };
    assert_eq!(
        decode_lambda_bootstrap(
            protocol.owner,
            protocol.name,
            protocol.descriptor,
            &field_handle
        ),
        Err(LambdaBootstrapError::UnadmittedReferenceKind(1))
    );
    let mut invalid_method_type = valid.to_vec();
    invalid_method_type[0] = ResolvedBootstrapArgument::MethodType("()garbage".into());
    assert!(matches!(
        decode_lambda_bootstrap(
            protocol.owner,
            protocol.name,
            protocol.descriptor,
            &invalid_method_type
        ),
        Err(LambdaBootstrapError::MalformedPayload(_))
    ));
}

#[test]
fn wrong_protocol_payload_stage_rejects_duplicates_conflicts_and_unknown_bits_exactly() {
    let protocol = &executor_admitted_lambda_protocols()[1];
    let fixed = [
        ResolvedBootstrapArgument::MethodType("()Ljava/lang/Object;".into()),
        ResolvedBootstrapArgument::MethodHandle { reference_kind: 6 },
        ResolvedBootstrapArgument::MethodType("()Ljava/lang/String;".into()),
    ];
    let decode = |tail: Vec<ResolvedBootstrapArgument>| {
        let arguments = fixed.iter().cloned().chain(tail).collect::<Vec<_>>();
        decode_lambda_bootstrap(
            protocol.owner,
            protocol.name,
            protocol.descriptor,
            &arguments,
        )
    };

    assert_eq!(
        decode(vec![ResolvedBootstrapArgument::Integer(8)]),
        Err(LambdaBootstrapError::MalformedPayload(
            "unknown altMetafactory flag bit 3".into()
        ))
    );
    assert_eq!(
        decode(vec![
            ResolvedBootstrapArgument::Integer(2),
            ResolvedBootstrapArgument::Integer(2),
            ResolvedBootstrapArgument::Class("example/Marker".into()),
            ResolvedBootstrapArgument::Class("example/Marker".into()),
        ]),
        Err(LambdaBootstrapError::MalformedPayload(
            "duplicate marker interface example/Marker".into()
        ))
    );
    assert_eq!(
        decode(vec![
            ResolvedBootstrapArgument::Integer(4),
            ResolvedBootstrapArgument::Integer(1),
            ResolvedBootstrapArgument::MethodType("()Ljava/lang/Object;".into()),
        ]),
        Err(LambdaBootstrapError::MalformedPayload(
            "bridge ()Ljava/lang/Object; conflicts with the SAM method".into()
        ))
    );
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
