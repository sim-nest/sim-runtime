use std::sync::Arc;

use sim_codec_classfile::{ByteReader, ConstantPool, Opcode, decode_instructions};
use sim_kernel::{CodecId, Cx, DefaultFactory, NoopEvalPolicy, Origin, SourceId, Span, Symbol};
use sim_lib_control::{Raised, WorkLimit};
use sim_lib_lang_jvm::{
    FailureCondition, JavaThrowable, JvmInstructionPolicy, JvmInstructionSemantics, JvmSlotKind,
    JvmValue, JvmValueWidth, NUMERIC_OWNERSHIP, NumericExecutionError, execute_numeric_instruction,
    prepare_code,
};
use sim_lib_machine::UnitStack;

const NONE: &[JvmSlotKind] = &[];
struct Policy;
impl JvmInstructionPolicy for Policy {
    fn semantics(_: Opcode) -> Option<JvmInstructionSemantics> {
        Some(JvmInstructionSemantics {
            pops: NONE,
            pushes: NONE,
            safepoint: false,
        })
    }
}
fn instruction(opcode: Opcode) -> sim_lib_lang_jvm::PreparedJvmInstruction {
    let pool = ConstantPool::decode(&mut ByteReader::new(&[0, 1], 1), 61).unwrap();
    let decoded = decode_instructions(&[opcode as u8], 61, &pool).unwrap();
    let code = prepare_code::<Policy>(&decoded, 1, &[], SourceId("numeric".into())).unwrap();
    code.instruction(code.entry()).instruction().clone()
}
fn run(opcode: Opcode, values: Vec<JvmValue>) -> Result<JvmValue, NumericExecutionError> {
    let mut stack = UnitStack::<JvmValueWidth>::new(WorkLimit(8));
    for value in values {
        stack.push(value).unwrap();
    }
    execute_numeric_instruction(&instruction(opcode), &mut stack, arithmetic_throwable)?;
    Ok(stack.pop().unwrap())
}
fn arithmetic_throwable() -> JavaThrowable {
    let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    let raised = Raised::new(
        cx.factory()
            .symbol(Symbol::new("java/lang/ArithmeticException"))
            .unwrap(),
        cx.factory().string("/ by zero".into()).unwrap(),
        Origin {
            codec: CodecId(0),
            source: SourceId("numeric".into()),
            span: Span { start: 0, end: 1 },
            trivia: vec![],
        },
        Symbol::new("java/jvm"),
    )
    .unwrap();
    JavaThrowable::new(FailureCondition::Arithmetic, raised).unwrap()
}

#[test]
fn normative_integer_and_throwable_edges() {
    assert!(matches!(
        run(
            Opcode::Idiv,
            vec![JvmValue::Int(i32::MIN), JvmValue::Int(-1)]
        ),
        Ok(JvmValue::Int(i32::MIN))
    ));
    let error = run(Opcode::Irem, vec![JvmValue::Int(1), JvmValue::Int(0)]).unwrap_err();
    assert!(
        matches!(error, NumericExecutionError::Arithmetic(ref thrown) if thrown.condition()==FailureCondition::Arithmetic)
    );
}

#[test]
fn normative_nan_comparisons_and_compare_variants() {
    let nan = JvmValue::Float(f32::NAN.to_bits());
    let one = JvmValue::Float(1.0f32.to_bits());
    for values in [
        vec![nan.clone(), one.clone()],
        vec![one.clone(), nan.clone()],
    ] {
        assert!(matches!(
            run(Opcode::Fcmpl, values.clone()),
            Ok(JvmValue::Int(-1))
        ));
        assert!(matches!(run(Opcode::Fcmpg, values), Ok(JvmValue::Int(1))));
    }
}

#[test]
fn normative_d2i_saturation_and_signed_zero() {
    assert!(matches!(
        run(Opcode::D2i, vec![JvmValue::Double(f64::INFINITY.to_bits())]),
        Ok(JvmValue::Int(i32::MAX))
    ));
    assert!(matches!(
        run(
            Opcode::D2i,
            vec![JvmValue::Double(f64::NEG_INFINITY.to_bits())]
        ),
        Ok(JvmValue::Int(i32::MIN))
    ));
    assert!(matches!(
        run(Opcode::D2i, vec![JvmValue::Double(f64::NAN.to_bits())]),
        Ok(JvmValue::Int(0))
    ));
    assert!(
        matches!(run(Opcode::Dneg, vec![JvmValue::Double(0.0f64.to_bits())]), Ok(JvmValue::Double(bits)) if bits == (-0.0f64).to_bits())
    );
}

#[test]
fn owner_table_has_no_empty_cell() {
    let table: toml::Value = toml::from_str(NUMERIC_OWNERSHIP).unwrap();
    for row in table["family"].as_array().unwrap() {
        for key in ["operations", "owner", "reason"] {
            assert!(!row[key].as_str().unwrap().trim().is_empty());
        }
    }
}
