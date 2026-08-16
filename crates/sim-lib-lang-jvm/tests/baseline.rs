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
