use sim_codec_classfile::{ClassfileCodec, inspect_classfile};
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

    let intrinsics: toml::Value = sim_lib_lang_jvm::INTRINSIC_MANIFEST.parse().unwrap();
    assert!(intrinsics["members"].as_array().unwrap().is_empty());

    let ledger: toml::Value = sim_lib_lang_jvm::REUSE_LEDGER.parse().unwrap();
    assert_eq!(ledger["organ"].as_array().unwrap().len(), 9);
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
