use std::cell::Cell;

use sim_kernel::{CodecId, Origin, SourceId, Span};
use sim_lib_control::WorkLimit;
use sim_lib_machine::{
    AdmissionLimits, AdmissionPolicy, DriveError, DriveOutcome, Driver, FrameStack,
    InstructionDriverPolicy, InstructionPolicy, LocatedCode, LocatedInstruction,
    MachineDescription, MachineFrame, MachinePermit, SourceLocation, StepKind, StepOutcome,
};

struct Instructions;

impl InstructionPolicy for Instructions {
    type Instruction = u8;
    type InstructionId = u8;

    fn instruction_id(instruction: &u8) -> u8 {
        *instruction
    }
}

struct Admission;

impl AdmissionPolicy<Instructions, ()> for Admission {
    type Refusal = ();

    fn validate_description(_: &MachineDescription<'_, Instructions, ()>) -> Result<(), ()> {
        Ok(())
    }

    fn validate_instruction(_: &u8, _: &()) -> Result<(), ()> {
        Ok(())
    }

    fn encode_metadata(_: &(), _: &mut Vec<u8>) {}

    fn encode_instruction(instruction: &u8, output: &mut Vec<u8>) {
        output.push(*instruction);
    }
}

#[derive(Clone)]
struct TestFrame {
    cursor: sim_lib_machine::CodeCursor,
    remaining_calls: usize,
    awaiting_return: bool,
}

impl MachineFrame for TestFrame {
    fn cursor(&self) -> sim_lib_machine::CodeCursor {
        self.cursor
    }

    fn set_cursor(&mut self, cursor: sim_lib_machine::CodeCursor) {
        self.cursor = cursor;
    }
}

thread_local! {
    static ACTIVE_POLICY_CALLS: Cell<usize> = const { Cell::new(0) };
    static MAX_POLICY_CALLS: Cell<usize> = const { Cell::new(0) };
}

struct DepthProbe;

impl InstructionDriverPolicy<Instructions, TestFrame> for DepthProbe {
    type Return = ();
    type Abrupt = ();
    type Yield = ();
    type Interrupt = ();
    type Fault = ();

    fn step(
        &mut self,
        _: &u8,
        frame: &mut TestFrame,
    ) -> Result<StepOutcome<TestFrame, (), (), (), ()>, ()> {
        struct Guard;
        impl Drop for Guard {
            fn drop(&mut self) {
                ACTIVE_POLICY_CALLS.with(|active| active.set(active.get() - 1));
            }
        }
        ACTIVE_POLICY_CALLS.with(|active| {
            let depth = active.get() + 1;
            active.set(depth);
            MAX_POLICY_CALLS.with(|maximum| maximum.set(maximum.get().max(depth)));
        });
        let _guard = Guard;

        if frame.remaining_calls == 0 || frame.awaiting_return {
            Ok(StepOutcome::Return(()))
        } else {
            frame.awaiting_return = true;
            Ok(StepOutcome::Call(TestFrame {
                cursor: frame.cursor,
                remaining_calls: frame.remaining_calls - 1,
                awaiting_return: false,
            }))
        }
    }
}

#[test]
fn deep_guest_calls_have_constant_measured_host_callback_depth_and_stable_receipts() {
    const GUEST_DEPTH: usize = 50_000;
    let code = code(1);
    let description = MachineDescription::new(&code, limits(GUEST_DEPTH * 2 + 1), &());
    let permit = MachinePermit::admit::<_, _, Admission>(&description).unwrap();

    let run = || {
        MAX_POLICY_CALLS.with(|maximum| maximum.set(0));
        let mut frames = FrameStack::new(WorkLimit(GUEST_DEPTH + 1));
        frames
            .push(TestFrame {
                cursor: code.entry(),
                remaining_calls: GUEST_DEPTH,
                awaiting_return: false,
            })
            .unwrap();
        let mut driver = Driver::new(DepthProbe);
        let outcome = driver
            .drive::<_, _, Admission, _>(
                &description,
                &permit,
                &mut frames,
                WorkLimit(GUEST_DEPTH * 2 + 1),
            )
            .unwrap();
        let receipt = match outcome {
            DriveOutcome::Return((), receipt) => receipt,
            _ => panic!("deep program did not return"),
        };
        assert_eq!(receipt.charged(), GUEST_DEPTH * 2 + 1);
        assert_eq!(receipt.steps()[0], (1, StepKind::Call));
        assert_eq!(receipt.steps()[GUEST_DEPTH], (1, StepKind::Return));
        assert_eq!(MAX_POLICY_CALLS.with(Cell::get), 1);
        receipt
    };

    assert_eq!(
        run(),
        run(),
        "fixed work receipts must be byte-for-byte stable"
    );
}

struct CountingPolicy<'a>(&'a Cell<usize>);

impl InstructionDriverPolicy<Instructions, TestFrame> for CountingPolicy<'_> {
    type Return = ();
    type Abrupt = ();
    type Yield = ();
    type Interrupt = ();
    type Fault = ();

    fn step(
        &mut self,
        _: &u8,
        _: &mut TestFrame,
    ) -> Result<StepOutcome<TestFrame, (), (), (), ()>, ()> {
        self.0.set(self.0.get() + 1);
        Ok(StepOutcome::Yield(()))
    }
}

#[test]
fn a_mismatched_permit_precedes_every_policy_effect() {
    let admitted_code = code(1);
    let admitted = MachineDescription::new(&admitted_code, limits(1), &());
    let permit = MachinePermit::admit::<_, _, Admission>(&admitted).unwrap();
    let edited_code = code(2);
    let edited = MachineDescription::new(&edited_code, limits(1), &());
    let callbacks = Cell::new(0);
    let mut driver = Driver::new(CountingPolicy(&callbacks));
    let mut frames = FrameStack::new(WorkLimit(1));
    frames
        .push(TestFrame {
            cursor: edited_code.entry(),
            remaining_calls: 0,
            awaiting_return: false,
        })
        .unwrap();

    assert!(matches!(
        driver.drive::<_, _, Admission, _>(&edited, &permit, &mut frames, WorkLimit(1)),
        Err(DriveError::PermitMismatch)
    ));
    assert_eq!(callbacks.get(), 0);
}

fn code(instruction: u8) -> LocatedCode<Instructions> {
    LocatedCode::freeze(
        vec![LocatedInstruction::new(
            instruction,
            instruction,
            SourceLocation::Bytes(Origin {
                codec: CodecId(1),
                source: SourceId("iterative-driver-test".into()),
                span: Span { start: 0, end: 1 },
                trivia: vec![],
            }),
            false,
            None,
        )],
        vec![],
        vec![],
    )
    .unwrap()
}

fn limits(work: usize) -> AdmissionLimits {
    AdmissionLimits {
        instructions: 1,
        operand_units: 1,
        slots: 1,
        frames: work,
        work,
    }
}
