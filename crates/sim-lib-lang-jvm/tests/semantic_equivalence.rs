use std::sync::Arc;

use sim_codec_classfile::{ByteReader, ConstantPool, InstructionId, Opcode, decode_instructions};
use sim_kernel::{Cx, Datum, DefaultFactory, NoopEvalPolicy, SourceId, Symbol};
use sim_lib_control::WorkLimit;
use sim_lib_gc_tracing::CollectionLimits;
use sim_lib_lang_jvm::{
    JVM_ROLE_EDGE_TABLE, JvmControlOutcome, JvmHeap, JvmInstructionPolicy, JvmInstructionSemantics,
    JvmRole, JvmSlotKind, JvmValue, JvmValueWidth, execute_control_instruction, prepare_code,
};
use sim_lib_machine::UnitStack;
use sim_lib_standard_core::{
    BoundedLane, CanonicalObservation, CanonicalOutcome, CaptureComparison,
    CaptureComparisonProjection, CharacterizationCapture, ScenarioLimits, ScenarioObservationLane,
    ScenarioSpec, compare_characterization_captures, publish_characterization_capture,
};

const NONE: &[JvmSlotKind] = &[];
const INT: &[JvmSlotKind] = &[JvmSlotKind::CategoryOne];
const PROGRAMS: &[(&str, &[u8])] = &[
    ("int-return", &[Opcode::Ireturn as u8]),
    (
        "fallthrough-return",
        &[Opcode::Ifeq as u8, 0, 3, Opcode::Return as u8],
    ),
    ("backward-interrupt", &[Opcode::Goto as u8, 0, 0]),
    ("void-return", &[Opcode::Return as u8]),
];

struct CorpusPolicy;

impl JvmInstructionPolicy for CorpusPolicy {
    fn semantics(opcode: Opcode) -> Option<JvmInstructionSemantics> {
        let (pops, pushes) = match opcode {
            Opcode::Iconst0 | Opcode::Iconst1 => (NONE, INT),
            Opcode::Ifeq => (INT, NONE),
            Opcode::Goto | Opcode::Return => (NONE, NONE),
            Opcode::Ireturn => (INT, NONE),
            _ => return None,
        };
        Some(JvmInstructionSemantics {
            pops,
            pushes,
            safepoint: false,
        })
    }
}

#[derive(Clone, Copy)]
struct PreparedVariant {
    work_bias: usize,
}

fn empty_pool() -> ConstantPool {
    ConstantPool::decode(&mut ByteReader::new(&[0, 1], 1), 61).unwrap()
}

fn scenario(name: &str) -> ScenarioSpec {
    ScenarioSpec::new(
        Symbol::qualified("jvm-bytecode-speed", name),
        Symbol::qualified("jvm-bytecode-speed", "prepared-equivalence/v1"),
    )
    .with_limits(ScenarioLimits::new(0, 64))
    .observing(ScenarioObservationLane::ValueOrFailure)
    .observing(ScenarioObservationLane::Events)
    .observing(ScenarioObservationLane::Receipts)
    .observing(ScenarioObservationLane::Browse)
}

fn node(tag: &str, fields: impl IntoIterator<Item = (&'static str, Datum)>) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("jvm-bytecode-speed", tag),
        fields: fields
            .into_iter()
            .map(|(key, value)| (Symbol::new(key), value))
            .collect(),
    }
}

fn capture(name: &str, bytes: &[u8], variant: PreparedVariant) -> CharacterizationCapture {
    let decoded = decode_instructions(bytes, 61, &empty_pool()).unwrap();
    let code =
        prepare_code::<CorpusPolicy>(&decoded, bytes.len(), &[], SourceId(name.into())).unwrap();
    let mut stack = UnitStack::<JvmValueWidth>::new(WorkLimit(64));
    if matches!(name, "int-return" | "fallthrough-return") {
        stack.push(JvmValue::Int(1)).unwrap();
    }
    let mut cursor = code.entry();
    let mut events = Vec::new();
    let mut receipts = Vec::new();
    let mut outcome = Datum::String("bounded-loop".into());

    for step in 0..8 {
        let interrupt = name == "backward-interrupt" && step == 0;
        match execute_control_instruction(&code, cursor, &mut stack, interrupt).unwrap() {
            JvmControlOutcome::Continue {
                cursor: next,
                receipt,
            } => {
                receipts.push(node(
                    "instruction-receipt",
                    [
                        (
                            "instruction",
                            Datum::String(receipt.instruction().0.to_string()),
                        ),
                        (
                            "work",
                            Datum::String((receipt.charged() + variant.work_bias).to_string()),
                        ),
                    ],
                ));
                events.push(Datum::String(format!(
                    "execute:{}",
                    receipt.instruction().0
                )));
                cursor = next;
            }
            JvmControlOutcome::Interrupted {
                instruction,
                cursor: resume,
            } => {
                events.push(Datum::String(format!("interrupt:{}", instruction.0)));
                receipts.push(node(
                    "interrupt-location",
                    [("instruction", Datum::String(instruction.0.to_string()))],
                ));
                cursor = resume;
            }
            JvmControlOutcome::Return { value, receipt } => {
                receipts.push(node(
                    "instruction-receipt",
                    [
                        (
                            "instruction",
                            Datum::String(receipt.instruction().0.to_string()),
                        ),
                        (
                            "work",
                            Datum::String((receipt.charged() + variant.work_bias).to_string()),
                        ),
                    ],
                ));
                outcome = match value {
                    Some(JvmValue::Int(value)) => Datum::String(format!("int:{value}")),
                    None => Datum::String("void".into()),
                    _ => Datum::String("other".into()),
                };
                break;
            }
        }
    }

    let limits = CollectionLimits {
        objects: 8,
        edges: 8,
        stack: 8,
        work: 64,
        clears: 8,
        finalizers: 0,
    };
    let mut heap = JvmHeap::new(2, limits).unwrap();
    let rooted = heap.allocate(JvmRole::Object).unwrap();
    let root = heap.root(rooted).unwrap();
    let retained = heap.collect().unwrap();
    heap.release_root(root).unwrap();
    let collected = heap.collect().unwrap();
    receipts.push(node(
        "root-set",
        [(
            "allocation",
            Datum::String(rooted.id().allocation_ordinal().to_string()),
        )],
    ));
    receipts.push(node(
        "collection",
        [
            ("retained", Datum::String(retained.swept.len().to_string())),
            ("swept", Datum::String(collected.swept.len().to_string())),
        ],
    ));
    let handler_memberships = (0..code.len())
        .map(|index| {
            code.instruction(code.cursor(InstructionId(index as u32)).unwrap())
                .instruction()
                .handler_membership()
                .len()
        })
        .sum::<usize>();
    receipts.push(node(
        "handler-paths",
        [(
            "memberships",
            Datum::String(handler_memberships.to_string()),
        )],
    ));

    CharacterizationCapture::new(
        Symbol::qualified("jvm-bytecode-speed", "semantic-equivalence/v1"),
        CanonicalObservation {
            outcome: Some(CanonicalOutcome::Success(outcome)),
            events: BoundedLane::Complete(events),
            receipts: BoundedLane::Complete(receipts),
            browse: BoundedLane::Complete(vec![node(
                "prepared-code",
                [
                    ("instructions", Datum::String(code.len().to_string())),
                    (
                        "role-kinds",
                        Datum::String(JVM_ROLE_EDGE_TABLE.len().to_string()),
                    ),
                ],
            )]),
        },
    )
}

fn compare(
    name: &str,
    bytes: &[u8],
    left: PreparedVariant,
    right: PreparedVariant,
) -> CaptureComparison {
    compare_characterization_captures(
        &scenario(name),
        &capture(name, bytes, left),
        &scenario(name),
        &capture(name, bytes, right),
        &CaptureComparisonProjection::new(Symbol::qualified(
            "jvm-bytecode-speed",
            "semantic-equivalence/v1",
        )),
    )
    .unwrap()
}

#[test]
fn every_performance_program_has_a_frozen_content_identified_capture() {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    for (name, bytes) in PROGRAMS {
        let first = capture(name, bytes, PreparedVariant { work_bias: 0 });
        let replay = capture(name, bytes, PreparedVariant { work_bias: 0 });
        assert_eq!(
            publish_characterization_capture(&mut cx, &scenario(name), &first).unwrap(),
            publish_characterization_capture(&mut cx, &scenario(name), &replay).unwrap(),
            "{name} must retain a stable content identity",
        );
    }
}

#[test]
fn prepared_variants_compare_identically_step_for_step_in_every_lane() {
    for (name, bytes) in PROGRAMS {
        assert!(
            compare(
                name,
                bytes,
                PreparedVariant { work_bias: 0 },
                PreparedVariant { work_bias: 0 }
            )
            .is_same(),
            "{name}"
        );
    }
}

#[test]
fn comparison_locates_an_injected_off_by_one_work_charge() {
    let comparison = compare(
        "int-return",
        PROGRAMS[0].1,
        PreparedVariant { work_bias: 0 },
        PreparedVariant { work_bias: 1 },
    );
    assert!(!comparison.is_same());
    assert!(
        comparison
            .differences
            .iter()
            .all(|difference| difference.path.contains(".work"))
    );
}
