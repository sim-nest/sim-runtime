use sim_codec_classfile::{ByteReader, ConstantPool, Opcode, decode_instructions};
use sim_kernel::SourceId;
use sim_lib_lang_jvm::{
    JvmInstructionPolicy, JvmInstructionSemantics, JvmSlotKind, PreparationError, PreparedDispatch,
    PreparedJvmInstruction, dispatch_prepared, prepare_code,
};

const NONE: &[JvmSlotKind] = &[];

struct Policy;

impl JvmInstructionPolicy for Policy {
    fn semantics(_opcode: Opcode) -> Option<JvmInstructionSemantics> {
        Some(JvmInstructionSemantics {
            pops: NONE,
            pushes: NONE,
            safepoint: false,
        })
    }
}

#[derive(Default)]
struct FamilyRecorder;

impl PreparedDispatch for FamilyRecorder {
    type Output = &'static str;

    fn storage(&mut self, _instruction: &PreparedJvmInstruction) -> Self::Output {
        "storage"
    }
    fn numeric(&mut self, _instruction: &PreparedJvmInstruction) -> Self::Output {
        "numeric"
    }
    fn control(&mut self, _instruction: &PreparedJvmInstruction) -> Self::Output {
        "control"
    }
    fn object(&mut self, _instruction: &PreparedJvmInstruction) -> Self::Output {
        "object"
    }
}

fn pool() -> ConstantPool {
    ConstantPool::decode(&mut ByteReader::new(&[0, 1], 1), 61).unwrap()
}

#[test]
fn prepared_dispatch_uses_generated_dense_family_identity() {
    let bytes = [Opcode::Nop as u8, Opcode::Iadd as u8, Opcode::Return as u8];
    let decoded = decode_instructions(&bytes, 61, &pool()).unwrap();
    let code =
        prepare_code::<Policy>(&decoded, bytes.len(), &[], SourceId("dense".into())).unwrap();
    let mut recorder = FamilyRecorder;
    let families: Vec<_> = (0..3)
        .map(|id| {
            dispatch_prepared(
                code.instruction(code.cursor(sim_codec_classfile::InstructionId(id)).unwrap())
                    .instruction(),
                &mut recorder,
            )
        })
        .collect();
    assert_eq!(families, ["storage", "numeric", "control"]);
}

#[test]
fn manifest_refusal_is_typed_during_preparation() {
    let bytes = [Opcode::Jsr as u8, 0, 0];
    let decoded = decode_instructions(&bytes, 61, &pool()).unwrap();
    let error = match prepare_code::<Policy>(&decoded, bytes.len(), &[], SourceId("refusal".into()))
    {
        Ok(_) => panic!("refused opcode was prepared"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        PreparationError::UnsupportedOpcode {
            opcode: Opcode::Jsr,
            offset: 0,
        }
    );
}

#[test]
fn drive_source_has_no_decode_or_manifest_lookup() {
    let source = include_str!("../src/dispatch.rs");
    assert!(!source.contains(concat!("decode_", "instructions(")));
    assert!(!source.contains(concat!(".meta", "data()")));
    assert!(!source.contains(concat!("OP", "CODES")));
}
