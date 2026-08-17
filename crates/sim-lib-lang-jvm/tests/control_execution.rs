use sim_codec_classfile::{ByteReader, ConstantPool, InstructionId, Opcode, decode_instructions};
use sim_kernel::SourceId;
use sim_lib_control::WorkLimit;
use sim_lib_lang_jvm::{
    JvmControlErrorKind, JvmControlOutcome, JvmInstructionPolicy, JvmInstructionSemantics,
    JvmSlotKind, JvmValue, JvmValueWidth, execute_control_instruction, prepare_code,
};
use sim_lib_machine::UnitStack;

const NONE: &[JvmSlotKind] = &[];
const INT: &[JvmSlotKind] = &[JvmSlotKind::CategoryOne];

struct ControlPolicy;

impl JvmInstructionPolicy for ControlPolicy {
    fn semantics(opcode: Opcode) -> Option<JvmInstructionSemantics> {
        use Opcode::*;
        let (pops, pushes) = match opcode {
            Iconst0 | Iconst1 => (NONE, INT),
            Goto => (NONE, NONE),
            Ifeq | Tableswitch | Lookupswitch => (INT, NONE),
            Ireturn => (INT, NONE),
            Return => (NONE, NONE),
            _ => return None,
        };
        Some(JvmInstructionSemantics {
            pops,
            pushes,
            safepoint: false,
        })
    }
}

fn empty_pool() -> ConstantPool {
    ConstantPool::decode(&mut ByteReader::new(&[0, 1], 1), 61).unwrap()
}

fn prepared(bytes: &[u8]) -> sim_lib_machine::LocatedCode<sim_lib_lang_jvm::PreparedJvmPolicy> {
    let decoded = decode_instructions(bytes, 61, &empty_pool()).unwrap();
    prepare_code::<ControlPolicy>(&decoded, bytes.len(), &[], SourceId("control".into())).unwrap()
}

fn stack() -> UnitStack<JvmValueWidth> {
    UnitStack::new(WorkLimit(16))
}

#[test]
fn backward_branch_interrupts_before_work_and_resumes_at_same_instruction() {
    let code = prepared(&[Opcode::Iconst0 as u8, Opcode::Goto as u8, 0xff, 0xff]);
    let cursor = code.cursor(InstructionId(1)).unwrap();
    assert!(code.instruction(cursor).is_safepoint());
    let mut operands = stack();

    match execute_control_instruction(&code, cursor, &mut operands, true).unwrap() {
        JvmControlOutcome::Interrupted {
            instruction,
            cursor: resume,
        } => {
            assert_eq!(instruction, InstructionId(1));
            assert_eq!(resume, cursor);
        }
        _ => panic!("backward edge must interrupt before executing"),
    }
    match execute_control_instruction(&code, cursor, &mut operands, false).unwrap() {
        JvmControlOutcome::Continue {
            cursor: target,
            receipt,
        } => {
            assert_eq!(target, code.cursor(InstructionId(0)).unwrap());
            assert_eq!(receipt.instruction(), InstructionId(1));
            assert_eq!(receipt.charged(), 1);
        }
        _ => panic!("resumed goto must complete exactly once"),
    }
}

#[test]
fn default_only_lookup_switch_executes_its_frozen_default() {
    let bytes = [
        Opcode::Lookupswitch as u8,
        0,
        0,
        0,
        0,
        0,
        0,
        12, // default -> return
        0,
        0,
        0,
        0, // zero pairs
        Opcode::Return as u8,
    ];
    let code = prepared(&bytes);
    let mut operands = stack();
    operands.push(JvmValue::Int(77)).unwrap();
    match execute_control_instruction(&code, code.entry(), &mut operands, false).unwrap() {
        JvmControlOutcome::Continue { cursor, receipt } => {
            assert_eq!(cursor, code.cursor(InstructionId(1)).unwrap());
            assert_eq!(receipt.charged(), 1);
            assert!(operands.is_empty());
        }
        _ => panic!("default-only switch must continue at its default"),
    }
}

#[test]
fn lookup_switch_accepts_the_full_signed_key_extremes() {
    let mut bytes = vec![Opcode::Lookupswitch as u8, 0, 0, 0];
    bytes.extend_from_slice(&29_i32.to_be_bytes());
    bytes.extend_from_slice(&2_i32.to_be_bytes());
    bytes.extend_from_slice(&i32::MIN.to_be_bytes());
    bytes.extend_from_slice(&28_i32.to_be_bytes());
    bytes.extend_from_slice(&i32::MAX.to_be_bytes());
    bytes.extend_from_slice(&29_i32.to_be_bytes());
    bytes.extend_from_slice(&[Opcode::Return as u8, Opcode::Return as u8]);
    let code = prepared(&bytes);

    for (key, expected) in [(i32::MIN, InstructionId(1)), (i32::MAX, InstructionId(2))] {
        let mut operands = stack();
        operands.push(JvmValue::Int(key)).unwrap();
        match execute_control_instruction(&code, code.entry(), &mut operands, false).unwrap() {
            JvmControlOutcome::Continue { cursor, .. } => {
                assert_eq!(cursor, code.cursor(expected).unwrap())
            }
            _ => panic!("extreme lookup key must branch"),
        }
    }
}

#[test]
fn table_switch_selects_range_endpoint_and_default() {
    let mut bytes = vec![Opcode::Tableswitch as u8, 0, 0, 0];
    bytes.extend_from_slice(&31_i32.to_be_bytes());
    bytes.extend_from_slice(&(-1_i32).to_be_bytes());
    bytes.extend_from_slice(&1_i32.to_be_bytes());
    for target in [28_i32, 29, 30] {
        bytes.extend_from_slice(&target.to_be_bytes());
    }
    bytes.extend_from_slice(&[
        Opcode::Return as u8,
        Opcode::Return as u8,
        Opcode::Return as u8,
        Opcode::Return as u8,
    ]);
    let code = prepared(&bytes);

    for (key, expected) in [(1, InstructionId(3)), (2, InstructionId(4))] {
        let mut operands = stack();
        operands.push(JvmValue::Int(key)).unwrap();
        match execute_control_instruction(&code, code.entry(), &mut operands, false).unwrap() {
            JvmControlOutcome::Continue { cursor, .. } => {
                assert_eq!(cursor, code.cursor(expected).unwrap())
            }
            _ => panic!("table switch must select a frozen target"),
        }
    }
}

#[test]
fn branch_fallthrough_return_and_boundary_failure_are_exact_and_located() {
    let code = prepared(&[
        Opcode::Ifeq as u8,
        0,
        4,
        Opcode::Ireturn as u8,
        Opcode::Return as u8,
    ]);
    let mut operands = stack();
    operands.push(JvmValue::Int(9)).unwrap();
    let next = match execute_control_instruction(&code, code.entry(), &mut operands, false).unwrap()
    {
        JvmControlOutcome::Continue { cursor, receipt } => {
            assert_eq!(receipt.instruction(), InstructionId(0));
            cursor
        }
        _ => panic!("false condition must fall through"),
    };
    operands.push(JvmValue::Int(42)).unwrap();
    match execute_control_instruction(&code, next, &mut operands, false).unwrap() {
        JvmControlOutcome::Return {
            value: Some(JvmValue::Int(42)),
            receipt,
        } => {
            assert_eq!(receipt.instruction(), InstructionId(1));
        }
        _ => panic!("ireturn must transfer exactly one int"),
    }

    let code = prepared(&[Opcode::Ifeq as u8, 0, 0]);
    let mut operands = stack();
    operands.push(JvmValue::Int(1)).unwrap();
    let error = execute_control_instruction(&code, code.entry(), &mut operands, false).unwrap_err();
    assert_eq!(error.instruction, InstructionId(0));
    assert_eq!(error.kind, JvmControlErrorKind::MissingFallthrough);
    assert_eq!(operands.depth(), 1);
    let sim_lib_machine::SourceLocation::Bytes(origin) = *error.location else {
        panic!()
    };
    assert_eq!(origin.span.start, 0);
}
