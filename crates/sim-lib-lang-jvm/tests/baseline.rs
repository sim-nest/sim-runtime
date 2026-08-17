// conformance: the bounded JVM baseline exercises every shared ownership seam.

use sim_codec_classfile::{ClassfileCodec, OPCODES, inspect_classfile};
use sim_kernel::CodecId;
use sim_lib_class::ClassDescriptor;
use sim_lib_control::Raised;
use sim_lib_core::SourceAuthority;
use sim_lib_machine::{InstructionPolicy, LocatedCode};
use sim_lib_mutation::ManagedNode;
use sim_text::CodeUnitString;

struct DependencyPolicy;

impl InstructionPolicy for DependencyPolicy {
    type Instruction = ();
    type InstructionId = u8;

    fn instruction_id(_: &Self::Instruction) -> Self::InstructionId {
        0
    }
}

#[test]
fn all_composed_organs_are_reachable() {
    fn reachable<T>() {
        assert!(!std::any::type_name::<T>().is_empty());
    }

    reachable::<Raised>();
    reachable::<ManagedNode<u64>>();
    reachable::<ClassDescriptor>();
    reachable::<SourceAuthority>();
    reachable::<CodeUnitString>();
    reachable::<LocatedCode<DependencyPolicy>>();
    reachable::<ClassfileCodec>();
}

#[test]
fn manifests_freeze_the_supported_baseline() {
    let supported: toml::Value = sim_lib_lang_jvm::SUPPORTED_RUNTIME.parse().unwrap();
    assert_eq!(
        supported["classfile_versions"]["minimum_major"].as_integer(),
        Some(45)
    );
    assert_eq!(
        supported["classfile_versions"]["maximum_major"].as_integer(),
        Some(69)
    );
    assert_eq!(supported["unsupported"].as_array().unwrap().len(), 5);
    assert_eq!(
        supported["unsupported"][1]["includes"]
            .as_array()
            .unwrap()
            .len(),
        4
    );

    let intrinsics: toml::Value = sim_lib_lang_jvm::INTRINSIC_MANIFEST.parse().unwrap();
    let manifest_members = intrinsics["members"].as_array().unwrap();
    assert_eq!(
        manifest_members.len(),
        sim_lib_lang_jvm::INTRINSIC_TABLE.len()
    );
    for (manifest, compiled) in manifest_members
        .iter()
        .zip(sim_lib_lang_jvm::INTRINSIC_TABLE)
    {
        assert_eq!(manifest["class"].as_str(), Some(compiled.class));
        assert_eq!(manifest["name"].as_str(), Some(compiled.name));
        assert_eq!(manifest["descriptor"].as_str(), Some(compiled.descriptor));
        assert_eq!(
            manifest["arguments_shape"].as_str(),
            Some(compiled.arguments_shape)
        );
        assert_eq!(
            manifest["result_shape"].as_str(),
            Some(compiled.result_shape)
        );
        assert_eq!(manifest["capability"].as_str(), Some(compiled.capability));
        assert_eq!(manifest["effect"].as_str(), Some(compiled.effect));
        assert_eq!(manifest["work"].as_integer(), Some(compiled.work.into()));
        assert_eq!(
            manifest["support"].as_str(),
            Some(match compiled.support {
                sim_lib_lang_jvm::IntrinsicSupport::Supported => "supported",
                sim_lib_lang_jvm::IntrinsicSupport::Unsupported => "unsupported",
            })
        );
    }

    let ledger: toml::Value = sim_lib_lang_jvm::REUSE_LEDGER.parse().unwrap();
    let products = ledger["organ"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["product"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        products,
        [
            "CHARACTERIZE_1",
            "INDEX_9",
            "MANAGED_2",
            "UTF16_2",
            "MACHINE_2",
            "CLASSFILE_2",
            "JVM_7",
            "DATAFLOW_2",
            "CLASS_2",
            "FUNCTION_2",
            "KERNEL",
            "DISPATCH",
            "CODECS",
            "EXCEPTIONS_3",
        ]
    );
}

#[test]
fn final_proof_is_wired_to_the_single_frozen_acceptance_file() {
    let reference: toml::Value = include_str!("../bytecode-speed-acceptance.toml")
        .parse()
        .unwrap();
    assert_eq!(reference["owner_repository"].as_str(), Some("sim-tooling"));
    assert_eq!(
        reference["path"].as_str(),
        Some("benchmarks/bytecode-speed-4/acceptance.toml")
    );
    assert_eq!(
        reference["final_proof_phase"].as_str(),
        Some("BYTECODESPEED4.14")
    );
}

#[test]
fn generated_coverage_differs_from_manifests_by_zero() {
    let intrinsics: toml::Value = sim_lib_lang_jvm::INTRINSIC_MANIFEST.parse().unwrap();
    let intrinsic_manifest_total = intrinsics["members"].as_array().unwrap().len();
    assert_eq!(
        intrinsic_manifest_total.abs_diff(sim_lib_lang_jvm::INTRINSIC_TABLE.len()),
        0
    );

    // OPCODES is itself generated from sim-codec-classfile's opcode-manifest.tsv;
    // JVM policy consumes that complete byte-indexed table instead of restating it.
    let opcode_manifest_total = OPCODES.len();
    let opcode_coverage_total = (u8::MIN..=u8::MAX)
        .filter(|byte| OPCODES[usize::from(*byte)].opcode as u8 == *byte)
        .count();
    assert_eq!(opcode_manifest_total.abs_diff(opcode_coverage_total), 0);
    assert_eq!(
        sim_lib_lang_jvm::VERIFIER_COVERAGE.opcode_rows,
        OPCODES.len()
    );
    assert_eq!(sim_lib_lang_jvm::VERIFIER_COVERAGE.rule_families, 5);
}

#[test]
fn verification_failures_are_readable_without_internal_state() {
    let explanation = sim_lib_lang_jvm::VerificationExplanation::for_method(
        &sim_lib_lang_jvm::MethodVerificationError::UnreachableHandler { row: 7 },
    );
    assert_eq!(explanation.code, "unreachable-handler");
    assert!(explanation.reason.contains("row 7"));
}

#[test]
fn verification_frames_have_bounded_read_only_views() {
    let mut frame =
        sim_lib_lang_jvm::VerificationFrame::new(sim_lib_lang_jvm::FrameKind::Locals, 3);
    frame
        .set_local(0, sim_lib_lang_jvm::VerificationType::Int)
        .unwrap();
    let view = sim_lib_lang_jvm::VerificationFrameView::bounded(&frame, 1);
    assert!(view.reachable);
    assert_eq!(view.capacity, 3);
    assert_eq!(view.slots.as_ref(), &[Some("Int".into())]);
    assert_eq!(view.omitted, 2);
}

#[test]
fn frozen_fixtures_decode_through_the_shared_classfile_organ() {
    let javac = include_bytes!("../fixtures/javac/StaticInt.class");
    assert_eq!(&javac[6..8], &52_u16.to_be_bytes());
    inspect_classfile(CodecId(139), javac.to_vec(), 4_096).unwrap();

    let hand_built = include_bytes!("../fixtures/hand-built/Minimal.class");
    assert_eq!(&hand_built[6..8], &45_u16.to_be_bytes());
    inspect_classfile(CodecId(139), hand_built.to_vec(), 4_096).unwrap();
}
