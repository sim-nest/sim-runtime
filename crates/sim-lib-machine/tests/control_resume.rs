// conformance: protected handlers, exactly-once cleanup, and content-bound resume.
use std::sync::{Arc, Mutex};

use sim_kernel::{CodecId, Origin, SourceId, Span};
use sim_lib_control::{CleanupStack, WorkLimit};
use sim_lib_machine::{
    AdmissionLimits, AdmissionPolicy, DriveOutcome, Driver, FrameStack, InstructionDriverPolicy,
    InstructionPolicy, LocatedCode, LocatedInstruction, MachineAbrupt, MachineCheckpoint,
    MachineDescription, MachineFrame, MachinePermit, MachineUnwind, RegionSpec, SourceLocation,
    StepOutcome, TargetLocation,
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct TestFrame {
    cursor: sim_lib_machine::CodeCursor,
}

impl MachineFrame for TestFrame {
    fn cursor(&self) -> sim_lib_machine::CodeCursor {
        self.cursor
    }

    fn set_cursor(&mut self, cursor: sim_lib_machine::CodeCursor) {
        self.cursor = cursor;
    }
}

struct Program<'a> {
    code: &'a LocatedCode<Instructions>,
    interrupt_once: bool,
    raise_once: bool,
}

impl InstructionDriverPolicy<Instructions, TestFrame> for Program<'_> {
    type Return = ();
    type Abrupt = &'static str;
    type Yield = ();
    type Interrupt = &'static str;
    type Fault = ();

    fn step(
        &mut self,
        instruction: &u8,
        frame: &mut TestFrame,
    ) -> Result<StepOutcome<TestFrame, (), &'static str, (), &'static str>, ()> {
        if *instruction == 3 && self.raise_once {
            self.raise_once = false;
            return Ok(StepOutcome::Raise("raised"));
        }
        if *instruction == 3 && self.interrupt_once {
            self.interrupt_once = false;
            frame.set_cursor(self.code.cursor(4).unwrap());
            return Ok(StepOutcome::Interrupt("interrupt"));
        }
        match self.code.next(frame.cursor()) {
            Some(next) => Ok(StepOutcome::Continue(next)),
            None => Ok(StepOutcome::Return(())),
        }
    }
}

#[test]
fn nested_protected_interrupt_cleans_once_and_resumes_with_identical_receipts() {
    let code = nested_code();
    let description = MachineDescription::new(&code, limits(), &());
    let permit = MachinePermit::admit::<_, _, Admission>(&description).unwrap();
    let cleanup_order = Arc::new(Mutex::new(Vec::new()));
    let mut cleanups: CleanupStack<MachineUnwind<(), &'static str, &'static str>> =
        CleanupStack::new();
    for region in ["outer", "middle", "inner"] {
        let observed = Arc::clone(&cleanup_order);
        cleanups.push(move |reason| {
            assert!(matches!(
                reason,
                sim_lib_control::Unwind::Exception(MachineAbrupt::Interrupt("interrupt"))
            ));
            observed.lock().unwrap().push(region);
        });
    }

    let mut frames = frame_stack(&code);
    let mut interrupted = Driver::new(Program {
        code: &code,
        interrupt_once: true,
        raise_once: false,
    });
    let prefix = match interrupted
        .drive_with_cleanup::<_, _, Admission, _>(
            &description,
            &permit,
            &mut frames,
            WorkLimit(3),
            cleanups,
        )
        .unwrap()
    {
        DriveOutcome::Interrupt("interrupt", receipt) => receipt,
        _ => panic!("program did not interrupt"),
    };
    assert_eq!(*cleanup_order.lock().unwrap(), ["inner", "middle", "outer"]);

    let checkpoint = MachineCheckpoint::new(frames, &permit, prefix);
    assert_eq!(checkpoint.evidence().content_id(), permit.content_id());
    let (mut resumed_frames, _) = checkpoint.resume(&permit).unwrap();
    let resumed_tail = match interrupted
        .drive_protected::<_, _, Admission, _>(
            &description,
            &permit,
            &mut resumed_frames,
            WorkLimit(6),
        )
        .unwrap()
    {
        DriveOutcome::Return((), receipt) => receipt,
        _ => panic!("resumed program did not return"),
    };

    let mut baseline_frames = frame_stack(&code);
    let mut uninterrupted = Driver::new(Program {
        code: &code,
        interrupt_once: false,
        raise_once: false,
    });
    let _prefix = uninterrupted
        .drive_protected::<_, _, Admission, _>(
            &description,
            &permit,
            &mut baseline_frames,
            WorkLimit(3),
        )
        .unwrap();
    let baseline_tail = match uninterrupted
        .drive_protected::<_, _, Admission, _>(
            &description,
            &permit,
            &mut baseline_frames,
            WorkLimit(4),
        )
        .unwrap()
    {
        DriveOutcome::Return((), receipt) => receipt,
        _ => panic!("baseline program did not return"),
    };
    assert_eq!(resumed_tail, baseline_tail);
}

#[test]
fn innermost_handler_is_selected_and_budget_exhaustion_cleans_once() {
    let code = nested_code();
    let description = MachineDescription::new(&code, limits(), &());
    let permit = MachinePermit::admit::<_, _, Admission>(&description).unwrap();
    let mut frames = frame_stack(&code);
    let mut driver = Driver::new(Program {
        code: &code,
        interrupt_once: false,
        raise_once: true,
    });
    let handled = driver
        .drive_protected::<_, _, Admission, _>(&description, &permit, &mut frames, WorkLimit(6))
        .unwrap();
    let receipt = match handled {
        DriveOutcome::Return((), receipt) => receipt,
        _ => panic!("innermost handler did not complete"),
    };
    assert_eq!(receipt.steps()[3].0, 5);
    assert_eq!(receipt.steps().last().unwrap().0, 7);

    let count = Arc::new(Mutex::new(0));
    let mut cleanups = CleanupStack::new();
    let observed = Arc::clone(&count);
    cleanups.push(
        move |reason: &MachineUnwind<(), &'static str, &'static str>| {
            assert!(matches!(
                reason,
                sim_lib_control::Unwind::Exception(MachineAbrupt::BudgetExhausted)
            ));
            *observed.lock().unwrap() += 1;
        },
    );
    let mut frames = frame_stack(&code);
    let mut bounded = Driver::new(Program {
        code: &code,
        interrupt_once: false,
        raise_once: false,
    });
    assert!(matches!(
        bounded
            .drive_with_cleanup::<_, _, Admission, _>(
                &description,
                &permit,
                &mut frames,
                WorkLimit(1),
                cleanups,
            )
            .unwrap(),
        DriveOutcome::Continue(_)
    ));
    assert_eq!(*count.lock().unwrap(), 1);
}

fn frame_stack(code: &LocatedCode<Instructions>) -> FrameStack<TestFrame> {
    let mut frames = FrameStack::new(WorkLimit(1));
    frames
        .push(TestFrame {
            cursor: code.entry(),
        })
        .unwrap();
    frames
}

fn nested_code() -> LocatedCode<Instructions> {
    let instructions = (1..=7)
        .map(|instruction| {
            LocatedInstruction::new(
                instruction,
                instruction,
                SourceLocation::Bytes(Origin {
                    codec: CodecId(1),
                    source: SourceId("control-resume-test".into()),
                    span: Span {
                        start: usize::from(instruction - 1),
                        end: usize::from(instruction),
                    },
                    trivia: vec![],
                }),
                false,
                None,
            )
        })
        .collect();
    LocatedCode::freeze(
        instructions,
        vec![],
        vec![region(1, 7, 7), region(2, 6, 6), region(3, 5, 5)],
    )
    .unwrap()
}

fn region(start: u8, end: u8, handler: u8) -> RegionSpec<u8> {
    RegionSpec {
        start,
        end: Some(end),
        handler: TargetLocation::Instruction(handler),
    }
}

fn limits() -> AdmissionLimits {
    AdmissionLimits {
        instructions: 7,
        operand_units: 1,
        slots: 1,
        frames: 1,
        work: 7,
    }
}
