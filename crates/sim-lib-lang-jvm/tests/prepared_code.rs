use sim_codec_classfile::{
    ByteReader, CodeException, ConstantPool, InstructionId, Opcode, decode_instructions,
};
use sim_kernel::{SourceId, Span};
use sim_lib_lang_jvm::{
    JvmInstructionPolicy, JvmInstructionSemantics, JvmSlotKind, PreparationError, prepare_code,
};
use sim_lib_machine::SourceLocation;

const NONE: &[JvmSlotKind] = &[];
const INT: &[JvmSlotKind] = &[JvmSlotKind::CategoryOne];

struct TinyPolicy;

fn empty_pool() -> ConstantPool {
    ConstantPool::decode(&mut ByteReader::new(&[0, 1], 1), 61).unwrap()
}

impl JvmInstructionPolicy for TinyPolicy {
    fn semantics(opcode: Opcode) -> Option<JvmInstructionSemantics> {
        match opcode {
            Opcode::Iconst0 => Some(JvmInstructionSemantics {
                pops: NONE,
                pushes: INT,
                safepoint: false,
            }),
            Opcode::Ireturn => Some(JvmInstructionSemantics {
                pops: INT,
                pushes: NONE,
                safepoint: false,
            }),
            Opcode::Goto => Some(JvmInstructionSemantics {
                pops: NONE,
                pushes: NONE,
                safepoint: true,
            }),
            _ => None,
        }
    }
}

#[test]
fn every_prepared_instruction_retains_exact_classfile_offset() {
    let bytes = [Opcode::Iconst0 as u8, Opcode::Ireturn as u8];
    let decoded = decode_instructions(&bytes, 61, &empty_pool()).unwrap();
    let prepared =
        prepare_code::<TinyPolicy>(&decoded, bytes.len(), &[], SourceId("Example.m()I".into()))
            .unwrap();

    let mut cursor = prepared.entry();
    for expected in [0, 1] {
        let SourceLocation::Bytes(origin) = prepared.instruction(cursor).location() else {
            panic!("JVM instructions must retain byte provenance")
        };
        assert_eq!(
            origin.span,
            Span {
                start: expected,
                end: expected + 1
            }
        );
        cursor = match prepared.next(cursor) {
            Some(next) => next,
            None => break,
        };
    }
    assert_eq!(
        prepared
            .instruction(prepared.entry())
            .instruction()
            .output_width(),
        1
    );
    assert_eq!(prepared.cursor(InstructionId(1)).unwrap(), cursor);
}

#[test]
fn admitted_but_unimplemented_opcode_is_named_during_preparation() {
    let bytes = [Opcode::Nop as u8];
    let decoded = decode_instructions(&bytes, 61, &empty_pool()).unwrap();
    let error = match prepare_code::<TinyPolicy>(&decoded, 1, &[], SourceId("missing".into())) {
        Ok(_) => panic!("nop has no TinyPolicy implementation"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        PreparationError::MissingInstructionPolicy {
            opcode: Opcode::Nop,
            mnemonic: "nop",
            offset: 0,
        }
    );
}

#[test]
fn handlers_and_branches_are_frozen_from_classfile_offsets() {
    let bytes = [Opcode::Iconst0 as u8, Opcode::Ireturn as u8];
    let decoded = decode_instructions(&bytes, 61, &empty_pool()).unwrap();
    let handlers = [CodeException {
        start_pc: 0,
        end_pc: 1,
        handler_pc: 1,
        catch_type: 0,
    }];
    let prepared =
        prepare_code::<TinyPolicy>(&decoded, bytes.len(), &handlers, SourceId("handler".into()))
            .unwrap();
    let membership = prepared
        .instruction(prepared.entry())
        .instruction()
        .handler_membership();
    assert_eq!(membership[0].row, 0);
    assert_eq!(membership[0].start, InstructionId(0));
    assert_eq!(membership[0].end, Some(InstructionId(1)));
    assert_eq!(membership[0].handler, InstructionId(1));
    assert_eq!(membership[0].catch_type, 0);
    let handler = prepared.cursor(InstructionId(1)).unwrap();
    assert_eq!(
        prepared
            .instruction(handler)
            .instruction()
            .handler_entries(),
        &[0]
    );
    assert!(prepared.protected_regions().is_empty());
}

#[test]
fn manifest_decoded_branch_displacements_resolve_to_machine_cursors() {
    let bytes = [
        Opcode::Goto as u8,
        0,
        3,
        Opcode::Iconst0 as u8,
        Opcode::Ireturn as u8,
    ];
    let decoded = decode_instructions(&bytes, 61, &empty_pool()).unwrap();
    let prepared =
        prepare_code::<TinyPolicy>(&decoded, bytes.len(), &[], SourceId("branch".into())).unwrap();
    let branch = prepared.instruction(prepared.entry());
    assert!(branch.is_safepoint());
    assert_eq!(
        prepared.branch_targets(InstructionId(0)),
        &[prepared.cursor(InstructionId(1)).unwrap()]
    );
}
