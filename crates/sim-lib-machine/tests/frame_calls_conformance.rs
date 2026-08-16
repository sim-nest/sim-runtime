// conformance: neutral machine frames preserve bounded call and resume behavior.

use sim_kernel::{CodecId, Origin, SourceId, Span};
use sim_lib_control::{AdmissionLimit, WorkLimit};
use sim_lib_machine::{
    CallTransfer, Frame, FrameStack, FrameStackError, InstructionPolicy, LocatedCode,
    LocatedInstruction, SourceLocation, TransferError, ValueWidthPolicy,
};

struct Instructions;

impl InstructionPolicy for Instructions {
    type Instruction = u8;
    type InstructionId = u8;

    fn instruction_id(instruction: &Self::Instruction) -> Self::InstructionId {
        *instruction
    }
}

struct Values;

impl ValueWidthPolicy for Values {
    type Value = u64;

    fn width(_: &Self::Value) -> usize {
        1
    }
}

fn entry_cursor() -> sim_lib_machine::CodeCursor {
    let code = LocatedCode::<Instructions>::freeze(
        vec![LocatedInstruction::new(
            0,
            0,
            SourceLocation::Bytes(Origin {
                codec: CodecId(1),
                source: SourceId("frame-test".into()),
                span: Span { start: 0, end: 1 },
                trivia: vec![],
            }),
            false,
            None,
        )],
        vec![],
        vec![],
    )
    .expect("single located instruction is valid");
    code.entry()
}

#[test]
fn million_deep_chain_exhausts_declared_budget_without_host_recursion() {
    type TestFrame = Frame<Values, (), (), ()>;
    const FRAME_BUDGET: usize = 32;
    const REQUESTED_DEPTH: usize = 1_000_000;

    let cursor = entry_cursor();
    let mut frames = FrameStack::<TestFrame>::new(WorkLimit(FRAME_BUDGET));
    let mut outcome = Ok(());
    for _ in 0..REQUESTED_DEPTH {
        outcome = frames.push(Frame::new(
            AdmissionLimit(2),
            WorkLimit(2),
            cursor,
            None,
            (),
            (),
        ));
        if outcome.is_err() {
            break;
        }
    }

    assert_eq!(
        outcome,
        Err(FrameStackError::DepthExhausted {
            depth: FRAME_BUDGET,
            limit: FRAME_BUDGET,
        })
    );
    assert_eq!(frames.depth(), FRAME_BUDGET);
}

#[test]
fn transfers_are_only_code_references_values_and_widths() {
    let packet = CallTransfer::new(vec![10_u64, 20], vec![1, 2], "code:sum")
        .expect("aligned nonzero widths");
    assert_eq!(packet.target, "code:sum");
    assert_eq!(packet.values, [10, 20]);
    assert_eq!(packet.widths, [1, 2]);
    assert_eq!(
        CallTransfer::new(vec![10_u64], vec![], "code:bad"),
        Err(TransferError::WidthCountMismatch)
    );

    let source = include_str!("../src/frame.rs").to_ascii_lowercase();
    for forbidden in ["method", "signature", "class"] {
        assert!(
            !source.contains(forbidden),
            "transfer/frame module imported forbidden guest concept: {forbidden}"
        );
    }
}
