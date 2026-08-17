use sim_kernel::{CodecId, Origin, SourceId, Span};
use sim_lib_machine::{
    BranchTarget, CodeError, CoverageMetadata, InstructionPolicy, LocatedCode, LocatedInstruction,
    RegionSpec, SourceLocation, TargetLocation,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct Instruction {
    id: u16,
}

struct Policy;

impl InstructionPolicy for Policy {
    type Instruction = Instruction;
    type InstructionId = u16;

    fn instruction_id(instruction: &Instruction) -> u16 {
        instruction.id
    }
}

#[test]
fn freezes_ids_locations_targets_regions_safepoints_and_coverage() {
    let code = LocatedCode::<Policy>::freeze(
        vec![
            instruction(10, 0, 2, false, None),
            instruction(20, 2, 5, true, Some(CoverageMetadata { counter: 7 })),
            instruction(30, 5, 6, false, None),
        ],
        vec![BranchTarget {
            from: 10,
            to: TargetLocation::Byte {
                source: source(),
                offset: 5,
            },
        }],
        vec![RegionSpec {
            start: 10,
            end: Some(20),
            handler: TargetLocation::Instruction(30),
        }],
    )
    .unwrap();

    let entry = code.entry();
    assert_eq!(code.instruction(entry).id(), &10);
    let second = code.next(entry).unwrap();
    assert!(code.instruction(second).is_safepoint());
    assert_eq!(
        code.instruction(second).coverage(),
        Some(CoverageMetadata { counter: 7 })
    );
    assert_eq!(code.branch_targets(10), &[code.cursor(30).unwrap()]);
    assert_eq!(
        code.protected_regions()[0].handler,
        code.cursor(30).unwrap()
    );
}

#[test]
fn branch_target_inside_instruction_names_branch_and_container() {
    let result = LocatedCode::<Policy>::freeze(
        vec![
            instruction(10, 0, 2, false, None),
            instruction(20, 2, 5, false, None),
        ],
        vec![BranchTarget {
            from: 10,
            to: TargetLocation::Byte {
                source: source(),
                offset: 3,
            },
        }],
        vec![],
    );

    assert!(matches!(
        result,
        Err(CodeError::InteriorTarget {
            from: 10,
            target: 3,
            containing: 20
        })
    ));
}

#[test]
fn validates_unreachable_locations_and_region_boundaries() {
    let malformed = LocatedCode::<Policy>::freeze(
        vec![
            instruction(10, 0, 2, false, None),
            instruction(20, 4, 4, false, None),
        ],
        vec![],
        vec![],
    );
    assert!(matches!(
        malformed,
        Err(CodeError::MalformedLocation {
            instruction: 20,
            start: 4,
            end: 4
        })
    ));

    let bad_handler = LocatedCode::<Policy>::freeze(
        vec![
            instruction(10, 0, 2, false, None),
            instruction(20, 2, 5, false, None),
        ],
        vec![],
        vec![RegionSpec {
            start: 10,
            end: Some(20),
            handler: TargetLocation::Byte {
                source: source(),
                offset: 3,
            },
        }],
    );
    assert!(matches!(
        bad_handler,
        Err(CodeError::InteriorTarget {
            from: 10,
            target: 3,
            containing: 20
        })
    ));
}

#[test]
fn token_locations_and_overlapping_regions_are_checked() {
    let instructions = vec![
        token_instruction(10, 0, 1),
        token_instruction(20, 1, 2),
        token_instruction(30, 2, 3),
    ];
    let result = LocatedCode::<Policy>::freeze(
        instructions,
        vec![],
        vec![
            RegionSpec {
                start: 10,
                end: Some(30),
                handler: TargetLocation::Token {
                    source: source(),
                    index: 2,
                },
            },
            RegionSpec {
                start: 20,
                end: None,
                handler: TargetLocation::Instruction(10),
            },
        ],
    );
    assert!(matches!(
        result,
        Err(CodeError::OverlappingRegions {
            first_start: 10,
            second_start: 20
        })
    ));
}

fn instruction(
    id: u16,
    start: usize,
    end: usize,
    safepoint: bool,
    coverage: Option<CoverageMetadata>,
) -> LocatedInstruction<Instruction, u16> {
    LocatedInstruction::new(
        Instruction { id },
        id,
        SourceLocation::Bytes(origin(start, end)),
        safepoint,
        coverage,
    )
}

fn token_instruction(id: u16, start: usize, end: usize) -> LocatedInstruction<Instruction, u16> {
    LocatedInstruction::new(
        Instruction { id },
        id,
        SourceLocation::Tokens {
            origin: origin(0, 10),
            start,
            end,
        },
        false,
        None,
    )
}

fn source() -> SourceId {
    SourceId("machine-test".into())
}

fn origin(start: usize, end: usize) -> Origin {
    Origin {
        codec: CodecId(1),
        source: source(),
        span: Span { start, end },
        trivia: vec![],
    }
}
