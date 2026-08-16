use std::sync::Arc;

use sim_codec_classfile::{ClassShell, ShellBudget, ShellErrorKind};
use sim_kernel::{CodecId, Cx, DefaultFactory, EagerPolicy, SourceId};
use sim_lib_lang_jvm::{JvmSurface, class_load_capability, jvm_invoke_capability};

const CORPUS: &str = include_str!("../fixtures/corpus.toml");
const JAVAC_STATIC_INT: &[u8] = include_bytes!("../fixtures/javac/StaticInt.class");
const HAND_BUILT_MINIMAL: &[u8] = include_bytes!("../fixtures/hand-built/Minimal.class");

fn authorized_cx() -> Cx {
    let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    for capability in [class_load_capability(), jvm_invoke_capability()] {
        seat.grant(&mut cx, capability).unwrap();
    }
    cx
}

fn decode(bytes: &[u8]) -> Result<ClassShell, sim_codec_classfile::ShellError> {
    ClassShell::decode(
        bytes,
        16_384,
        ShellBudget {
            interfaces: 64,
            fields: 64,
            methods: 64,
            attributes: 64,
            attribute_bytes: 16_384,
        },
        CodecId(139),
        SourceId("jvm-characterization-corpus".into()),
    )
}

#[test]
fn corpus_manifest_declares_every_normalization_at_the_scenario() {
    let corpus: toml::Value = CORPUS.parse().unwrap();
    assert_eq!(
        corpus["schema"].as_str(),
        Some("sim.jvm-characterization/v1")
    );
    let scenarios = corpus["scenario"].as_array().unwrap();
    assert_eq!(scenarios.len(), 5);
    for scenario in scenarios {
        assert!(
            scenario
                .get("expected")
                .and_then(toml::Value::as_str)
                .is_some()
        );
        assert!(
            scenario
                .get("normalizations")
                .and_then(toml::Value::as_array)
                .is_some(),
            "{} must own its normalization declarations",
            scenario["id"].as_str().unwrap()
        );
    }
    assert_eq!(corpus["finding"].as_array().unwrap().len(), 2);
}

#[test]
fn retained_positive_and_differential_corpus_matches_exact_guest_values() {
    let mut cx = authorized_cx();
    let surface = JvmSurface::new(16_384);
    surface
        .define(&mut cx, "StaticInt", JAVAC_STATIC_INT.to_vec())
        .unwrap();
    surface
        .define(&mut cx, "Minimal", HAND_BUILT_MINIMAL.to_vec())
        .unwrap();

    assert_eq!(
        surface
            .invoke_static_i32(&mut cx, "StaticInt", "wholePipeline", "(II)I", &[3, 4])
            .unwrap(),
        14
    );
    assert_eq!(
        surface
            .invoke_static_i32(&mut cx, "Minimal", "value", "()I", &[])
            .unwrap_err()
            .to_string(),
        "evaluation error: JVM callable subset refuses opcode Bipush"
    );
}

#[test]
fn malformed_corpus_preserves_failure_kinds_without_text_normalization() {
    let mut invalid_magic = HAND_BUILT_MINIMAL.to_vec();
    invalid_magic[..4].fill(0);
    assert_eq!(
        decode(&invalid_magic).unwrap_err().kind,
        ShellErrorKind::Magic
    );
    assert_eq!(
        decode(&HAND_BUILT_MINIMAL[..3]).unwrap_err().kind,
        ShellErrorKind::Bytes
    );
}

#[test]
fn runtime_negative_corpus_preserves_the_missing_method_refusal() {
    let mut cx = authorized_cx();
    let surface = JvmSurface::new(16_384);
    surface
        .define(&mut cx, "StaticInt", JAVAC_STATIC_INT.to_vec())
        .unwrap();
    let error = surface
        .invoke_static_i32(&mut cx, "StaticInt", "absent", "()I", &[])
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "evaluation error: missing JVM method StaticInt.absent()I"
    );
}
