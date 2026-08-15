use sim_kernel::{CodecId, Origin, SourceId, Span};
use sim_lib_control::AdmissionLimit;
use sim_lib_control::WorkLimit;
use sim_lib_machine::{
    AdmissionLimits, AdmissionPolicy, DriveOutcome, Driver, Frame, FrameStack,
    InstructionDriverPolicy, InstructionPolicy, LocatedCode, LocatedInstruction,
    MachineDescription, MachineFrame, MachinePermit, ManagedRootSource, RootScanError,
    RootSnapshot, SafepointDriveError, SourceLocation, StepOutcome, ValueWidthPolicy,
};
use sim_lib_mutation::{HardCappedRetainPolicy, ManagedArena, ManagedId, ManagedNode};

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
struct RootsFrame {
    cursor: sim_lib_machine::CodeCursor,
    slot_roots: Vec<ManagedId>,
    stack_roots: Vec<ManagedId>,
    frame_roots: Vec<ManagedId>,
    policy_roots: Vec<ManagedId>,
}

impl MachineFrame for RootsFrame {
    fn cursor(&self) -> sim_lib_machine::CodeCursor {
        self.cursor
    }
    fn set_cursor(&mut self, cursor: sim_lib_machine::CodeCursor) {
        self.cursor = cursor;
    }
}

impl ManagedRootSource for RootsFrame {
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        for roots in [
            &self.slot_roots,
            &self.stack_roots,
            &self.frame_roots,
            &self.policy_roots,
        ] {
            if !roots.visit_managed_roots(visit) {
                return false;
            }
        }
        true
    }
}

struct Yield;
impl InstructionDriverPolicy<Instructions, RootsFrame> for Yield {
    type Return = ();
    type Abrupt = ();
    type Yield = ();
    type Interrupt = ();
    type Fault = ();
    fn step(
        &mut self,
        _: &u8,
        _: &mut RootsFrame,
    ) -> Result<StepOutcome<RootsFrame, (), (), (), ()>, ()> {
        Ok(StepOutcome::Yield(()))
    }
}

struct RootValues;
impl ValueWidthPolicy for RootValues {
    type Value = Vec<ManagedId>;
    fn width(_: &Self::Value) -> usize {
        1
    }
}

#[test]
fn concrete_frame_projects_slots_operands_continuation_roots_and_policy_state() {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(5).unwrap());
    let ids = (0..5)
        .map(|_| arena.allocate(ManagedNode::new(())).unwrap().id())
        .collect::<Vec<_>>();
    let cursor = code(1, true).entry();
    let mut frame = Frame::<RootValues, _, _, _>::new(
        AdmissionLimit(1),
        WorkLimit(1),
        cursor,
        Some(ids[2]),
        vec![ids[3]],
        ids[4],
    );
    frame.slots_mut().store(0, vec![ids[0]]).unwrap();
    frame.operands_mut().push(vec![ids[1]]).unwrap();
    assert_eq!(
        RootSnapshot::scan(&frame, WorkLimit(5)).unwrap().roots(),
        ids
    );
}

#[test]
fn suspended_multiframe_roots_are_complete_at_every_declared_safepoint() {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(9).unwrap());
    let handles = (0..9)
        .map(|_| arena.allocate(ManagedNode::new(())).unwrap())
        .collect::<Vec<_>>();
    let ids = handles.iter().map(|handle| handle.id()).collect::<Vec<_>>();
    let expected = ids[..8].to_vec();

    for instruction in [1, 2, 3] {
        let code = code(instruction, true);
        let description = MachineDescription::new(&code, limits(), &());
        let permit = MachinePermit::admit::<_, _, Admission>(&description).unwrap();
        let mut frames = FrameStack::new(WorkLimit(2));
        frames.push(frame(code.entry(), &ids[..4])).unwrap();
        frames.push(frame(code.entry(), &ids[4..8])).unwrap();
        let mut observed = Vec::new();
        let outcome = Driver::new(Yield)
            .drive_with_safepoints::<_, _, Admission, _, ()>(
                &description,
                &permit,
                &mut frames,
                WorkLimit(1),
                WorkLimit(8),
                |roots| {
                    observed.push(roots.roots().to_vec());
                    Ok(())
                },
            )
            .unwrap();
        assert!(matches!(outcome, DriveOutcome::Yield((), _)));
        assert_eq!(observed.as_slice(), std::slice::from_ref(&expected));
        assert_eq!(reference_trace(observed[0].as_slice()), expected);
        assert!(
            !observed[0].contains(&ids[8]),
            "unreachable reference object entered the root set"
        );
    }
}

#[test]
fn root_and_instruction_work_exhaustion_are_deterministic() {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(2).unwrap());
    let ids = (0..2)
        .map(|_| arena.allocate(ManagedNode::new(())).unwrap().id())
        .collect::<Vec<_>>();
    let roots = frame(code(1, true).entry(), &ids);
    let first = RootSnapshot::scan(&roots, WorkLimit(1)).unwrap_err();
    let second = RootSnapshot::scan(&roots, WorkLimit(1)).unwrap_err();
    assert_eq!(first, second);
    assert_eq!(
        first,
        RootScanError::BudgetExhausted {
            visited: 1,
            limit: 1
        }
    );

    let code = code(1, true);
    let description = MachineDescription::new(&code, limits(), &());
    let permit = MachinePermit::admit::<_, _, Admission>(&description).unwrap();
    let run = || {
        let mut frames = FrameStack::new(WorkLimit(1));
        frames.push(frame(code.entry(), &ids)).unwrap();
        Driver::new(Yield).drive_with_safepoints::<_, _, Admission, _, ()>(
            &description,
            &permit,
            &mut frames,
            WorkLimit(1),
            WorkLimit(1),
            |_| Ok(()),
        )
    };
    assert!(matches!(
        run(),
        Err(SafepointDriveError::RootScan(
            RootScanError::BudgetExhausted {
                visited: 1,
                limit: 1
            }
        ))
    ));
    assert!(matches!(
        run(),
        Err(SafepointDriveError::RootScan(
            RootScanError::BudgetExhausted {
                visited: 1,
                limit: 1
            }
        ))
    ));

    let mut frames = FrameStack::new(WorkLimit(1));
    frames.push(frame(code.entry(), &ids)).unwrap();
    let outcome = Driver::new(Yield)
        .drive_with_safepoints::<_, _, Admission, _, ()>(
            &description,
            &permit,
            &mut frames,
            WorkLimit(0),
            WorkLimit(2),
            |_| Ok(()),
        )
        .unwrap();
    assert!(matches!(outcome, DriveOutcome::Continue(receipt) if receipt.steps().is_empty()));
}

fn reference_trace(roots: &[ManagedId]) -> Vec<ManagedId> {
    // This specimen has no object edges, so the independent fixed-point model's
    // reachable set is exactly the stable, deduplicated root frontier.
    let mut reachable = roots.to_vec();
    reachable.sort();
    reachable.dedup();
    reachable
}

fn frame(cursor: sim_lib_machine::CodeCursor, ids: &[ManagedId]) -> RootsFrame {
    RootsFrame {
        cursor,
        slot_roots: ids.first().copied().into_iter().collect(),
        stack_roots: ids.get(1).copied().into_iter().collect(),
        frame_roots: ids.get(2).copied().into_iter().collect(),
        policy_roots: ids.get(3).copied().into_iter().collect(),
    }
}

fn code(instruction: u8, safepoint: bool) -> LocatedCode<Instructions> {
    LocatedCode::freeze(
        vec![LocatedInstruction::new(
            instruction,
            instruction,
            SourceLocation::Bytes(Origin {
                codec: CodecId(1),
                source: SourceId("managed-safepoint-test".into()),
                span: Span { start: 0, end: 1 },
                trivia: vec![],
            }),
            safepoint,
            None,
        )],
        vec![],
        vec![],
    )
    .unwrap()
}

fn limits() -> AdmissionLimits {
    AdmissionLimits {
        instructions: 1,
        operand_units: 8,
        slots: 8,
        frames: 2,
        work: 8,
    }
}
