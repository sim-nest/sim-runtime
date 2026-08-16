use sim_codec_classfile::{ByteReader, ConstantPool, InstructionId, Opcode, decode_instructions};
use sim_kernel::SourceId;
use sim_lib_lang_jvm::{
    JvmInstructionPolicy, JvmInstructionSemantics, JvmSlotKind, PreparedJvmPolicy,
    PreparedWorkRegions, prepare_code,
};
use sim_lib_machine::LocatedCode;

const NONE: &[JvmSlotKind] = &[];
const INT: &[JvmSlotKind] = &[JvmSlotKind::CategoryOne];

struct Policy;

impl JvmInstructionPolicy for Policy {
    fn semantics(opcode: Opcode) -> Option<JvmInstructionSemantics> {
        let (pops, pushes, safepoint) = match opcode {
            Opcode::Iconst0 | Opcode::Iconst1 => (NONE, INT, false),
            Opcode::Pop => (INT, NONE, false),
            Opcode::Goto => (NONE, NONE, false),
            Opcode::Return => (NONE, NONE, false),
            _ => return None,
        };
        Some(JvmInstructionSemantics {
            pops,
            pushes,
            safepoint,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Stop {
    Exhausted(InstructionId),
    Interrupted(InstructionId),
    Complete,
}

#[derive(Debug, Eq, PartialEq)]
struct Trace {
    receipts: Vec<(InstructionId, usize)>,
    stop: Stop,
}

fn code(bytes: &[u8]) -> LocatedCode<PreparedJvmPolicy> {
    let pool = ConstantPool::decode(&mut ByteReader::new(&[0, 1], 1), 61).unwrap();
    let decoded = decode_instructions(bytes, 61, &pool).unwrap();
    prepare_code::<Policy>(&decoded, bytes.len(), &[], SourceId("work-regions".into())).unwrap()
}

fn trace(
    code: &LocatedCode<PreparedJvmPolicy>,
    regions: Option<&PreparedWorkRegions>,
    budget: usize,
    interrupt: Option<InstructionId>,
) -> Trace {
    let mut index = 0;
    let mut remaining = budget;
    let mut receipts = Vec::new();
    while index < code.len() {
        let cursor = code.cursor(InstructionId(index as u32)).unwrap();
        let located = code.instruction(cursor);
        let id = *located.id();
        if interrupt == Some(id) && located.is_safepoint() {
            return Trace {
                receipts,
                stop: Stop::Interrupted(id),
            };
        }
        let window = regions.map_or(1, |regions| {
            regions.execution_window(code, cursor, remaining, interrupt.is_some())
        });
        for _ in 0..window {
            let cursor = code.cursor(InstructionId(index as u32)).unwrap();
            let instruction = code.instruction(cursor).instruction();
            let charge = instruction.work_charge();
            if charge > remaining {
                return Trace {
                    receipts,
                    stop: Stop::Exhausted(instruction.id()),
                };
            }
            remaining -= charge;
            receipts.push((instruction.id(), charge));
            index += 1;
        }
    }
    Trace {
        receipts,
        stop: Stop::Complete,
    }
}

#[test]
fn regions_precompute_aggregate_work_between_semantic_boundaries() {
    let code = code(&[
        Opcode::Iconst0 as u8,
        Opcode::Pop as u8,
        Opcode::Goto as u8,
        0,
        3,
        Opcode::Return as u8,
    ]);
    let regions = PreparedWorkRegions::prepare(&code);
    assert_eq!(regions.regions().len(), 2);
    assert_eq!(regions.regions()[0].instruction_count(), 3);
    assert_eq!(regions.regions()[0].aggregate_charge(), 3);
    assert_eq!(regions.regions()[1].instruction_count(), 1);
}

#[test]
fn batched_receipts_and_stop_locations_match_baseline_for_the_corpus() {
    let corpus: &[&[u8]] = &[
        &[Opcode::Return as u8],
        &[
            Opcode::Iconst0 as u8,
            Opcode::Pop as u8,
            Opcode::Return as u8,
        ],
        &[Opcode::Goto as u8, 0, 0],
    ];
    for bytes in corpus {
        let code = code(bytes);
        let regions = PreparedWorkRegions::prepare(&code);
        for budget in 0..=code.len() + 1 {
            assert_eq!(
                trace(&code, None, budget, None),
                trace(&code, Some(&regions), budget, None),
                "budget {budget} for {bytes:?}",
            );
        }
        for index in 0..code.len() {
            let interrupt = Some(InstructionId(index as u32));
            assert_eq!(
                trace(&code, None, code.len() + 1, interrupt),
                trace(&code, Some(&regions), code.len() + 1, interrupt),
                "interrupt {index} for {bytes:?}",
            );
        }
    }
}

#[test]
fn budget_expiring_mid_region_stops_at_the_exact_instruction() {
    let code = code(&[
        Opcode::Iconst0 as u8,
        Opcode::Pop as u8,
        Opcode::Return as u8,
    ]);
    let regions = PreparedWorkRegions::prepare(&code);
    let expected = Trace {
        receipts: vec![(InstructionId(0), 1)],
        stop: Stop::Exhausted(InstructionId(1)),
    };
    assert_eq!(trace(&code, None, 1, None), expected);
    assert_eq!(trace(&code, Some(&regions), 1, None), expected);
}
