use sim_codec_classfile::{ByteReader, ConstantPool, Opcode, decode_instructions};
use sim_kernel::SourceId;
use sim_lib_control::{AdmissionLimit, WorkLimit};
use sim_lib_lang_jvm::{
    ExecutionError, JvmInstructionPolicy, JvmInstructionSemantics, JvmSlotKind, JvmValue,
    JvmValueWidth, execute_storage_instruction, prepare_code,
};
use sim_lib_machine::{SlotFile, UnitStack};

const NONE: &[JvmSlotKind] = &[];
struct StoragePolicy;
impl JvmInstructionPolicy for StoragePolicy {
    fn semantics(_: Opcode) -> Option<JvmInstructionSemantics> {
        Some(JvmInstructionSemantics {
            pops: NONE,
            pushes: NONE,
            safepoint: false,
        })
    }
}

fn empty_pool() -> ConstantPool {
    ConstantPool::decode(&mut ByteReader::new(&[0, 1], 1), 61).unwrap()
}

fn prepared(bytes: &[u8]) -> sim_lib_machine::LocatedCode<sim_lib_lang_jvm::PreparedJvmPolicy> {
    let decoded = decode_instructions(bytes, 61, &empty_pool()).unwrap();
    prepare_code::<StoragePolicy>(&decoded, bytes.len(), &[], SourceId("storage".into())).unwrap()
}

fn stack(values: &[JvmValue]) -> UnitStack<JvmValueWidth> {
    let mut stack = UnitStack::new(WorkLimit(16));
    for value in values {
        stack.push(value.clone()).unwrap();
    }
    stack
}

fn ints(stack: &UnitStack<JvmValueWidth>) -> Vec<i32> {
    let mut values = Vec::new();
    stack.visit_values(|value| {
        values.push(match value {
            JvmValue::Int(value) => *value,
            JvmValue::Long(value) => i32::try_from(*value).unwrap(),
            _ => panic!("unexpected test value"),
        })
    });
    values
}

#[test]
fn dup2_x2_uses_all_four_validated_category_layouts() {
    let code = prepared(&[Opcode::Dup2X2 as u8]);
    let instruction = code.instruction(code.entry()).instruction();
    let cases = [
        (vec![JvmValue::Long(1), JvmValue::Long(2)], vec![2, 1, 2]),
        (
            vec![JvmValue::Int(1), JvmValue::Int(2), JvmValue::Long(3)],
            vec![3, 1, 2, 3],
        ),
        (
            vec![JvmValue::Long(1), JvmValue::Int(2), JvmValue::Int(3)],
            vec![2, 3, 1, 2, 3],
        ),
        (
            vec![
                JvmValue::Int(1),
                JvmValue::Int(2),
                JvmValue::Int(3),
                JvmValue::Int(4),
            ],
            vec![3, 4, 1, 2, 3, 4],
        ),
    ];
    for (input, expected) in cases {
        let mut operands = stack(&input);
        let mut locals = SlotFile::new(AdmissionLimit(4));
        let receipt = execute_storage_instruction(
            instruction,
            &mut locals,
            &mut operands,
            &mut |_| unreachable!(),
        )
        .unwrap();
        assert_eq!(ints(&operands), expected);
        assert_eq!(receipt.charged(), 1);
    }
}

#[test]
fn every_pop_dup_and_swap_form_selects_a_whole_value_plan() {
    let cases = [
        (Opcode::Pop, vec![JvmValue::Int(1)], vec![]),
        (Opcode::Pop2, vec![JvmValue::Long(1)], vec![]),
        (
            Opcode::Pop2,
            vec![JvmValue::Int(1), JvmValue::Int(2)],
            vec![],
        ),
        (Opcode::Dup, vec![JvmValue::Int(1)], vec![1, 1]),
        (
            Opcode::DupX1,
            vec![JvmValue::Int(1), JvmValue::Int(2)],
            vec![2, 1, 2],
        ),
        (
            Opcode::DupX2,
            vec![JvmValue::Long(1), JvmValue::Int(2)],
            vec![2, 1, 2],
        ),
        (
            Opcode::DupX2,
            vec![JvmValue::Int(1), JvmValue::Int(2), JvmValue::Int(3)],
            vec![3, 1, 2, 3],
        ),
        (Opcode::Dup2, vec![JvmValue::Long(1)], vec![1, 1]),
        (
            Opcode::Dup2,
            vec![JvmValue::Int(1), JvmValue::Int(2)],
            vec![1, 2, 1, 2],
        ),
        (
            Opcode::Dup2X1,
            vec![JvmValue::Int(1), JvmValue::Long(2)],
            vec![2, 1, 2],
        ),
        (
            Opcode::Dup2X1,
            vec![JvmValue::Int(1), JvmValue::Int(2), JvmValue::Int(3)],
            vec![2, 3, 1, 2, 3],
        ),
        (
            Opcode::Swap,
            vec![JvmValue::Int(1), JvmValue::Int(2)],
            vec![2, 1],
        ),
    ];
    for (opcode, input, expected) in cases {
        let code = prepared(&[opcode as u8]);
        let mut operands = stack(&input);
        execute_storage_instruction(
            code.instruction(code.entry()).instruction(),
            &mut SlotFile::new(AdmissionLimit(1)),
            &mut operands,
            &mut |_| unreachable!(),
        )
        .unwrap();
        assert_eq!(ints(&operands), expected, "{opcode:?}");
    }
}

#[test]
fn constants_locals_and_iinc_round_trip_through_machine_storage() {
    let bytes = [
        Opcode::Bipush as u8,
        41,
        Opcode::Istore0 as u8,
        Opcode::Iinc as u8,
        0,
        1,
        Opcode::Iload0 as u8,
    ];
    let code = prepared(&bytes);
    let mut locals = SlotFile::new(AdmissionLimit(2));
    let mut operands = stack(&[]);
    let mut cursor = code.entry();
    loop {
        execute_storage_instruction(
            code.instruction(cursor).instruction(),
            &mut locals,
            &mut operands,
            &mut |_| unreachable!(),
        )
        .unwrap();
        let Some(next) = code.next(cursor) else { break };
        cursor = next;
    }
    assert_eq!(ints(&operands), vec![42]);
}

#[test]
fn malformed_category_layout_is_refused_without_work_or_mutation() {
    let code = prepared(&[Opcode::Swap as u8]);
    let mut operands = stack(&[JvmValue::Long(7), JvmValue::Int(8)]);
    let before = ints(&operands);
    let error = execute_storage_instruction(
        code.instruction(code.entry()).instruction(),
        &mut SlotFile::new(AdmissionLimit(1)),
        &mut operands,
        &mut |_| unreachable!(),
    )
    .unwrap_err();
    assert_eq!(
        error,
        ExecutionError::MalformedPreparedInput {
            opcode: Opcode::Swap
        }
    );
    assert_eq!(ints(&operands), before);
}
