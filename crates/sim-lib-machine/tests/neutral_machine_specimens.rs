// conformance: shared machine organs support stack and register specimens.

//! Neutral end-to-end specimens: one operand-stack machine and one register machine.

use sim_kernel::{CodecId, Origin, SourceId, Span};
use sim_lib_control::{AdmissionLimit, WorkLimit};
use sim_lib_machine::{
    AdmissionLimits, AdmissionPolicy, DriveOutcome, Driver, FrameStack, InstructionDriverPolicy,
    InstructionPolicy, LocatedCode, LocatedInstruction, MachineCheckpoint, MachineDescription,
    MachineFrame, MachinePermit, ManagedRootSource, RegionSpec, RootSnapshot, SlotError, SlotFile,
    SourceLocation, StepKind, StepOutcome, TargetLocation, UnitStack, ValueWidthPolicy,
};
use sim_lib_mutation::{HardCappedRetainPolicy, ManagedArena, ManagedId, ManagedNode};

#[derive(Clone, Copy)]
struct Instruction {
    id: u8,
    operation: Operation,
}

#[derive(Clone, Copy)]
enum Operation {
    Constant(i64),
    Divide,
    Add,
    Call(u8),
    Interrupt,
    Return,
}

struct Instructions;

impl InstructionPolicy for Instructions {
    type Instruction = Instruction;
    type InstructionId = u8;

    fn instruction_id(instruction: &Instruction) -> u8 {
        instruction.id
    }
}

struct Admission;

impl AdmissionPolicy<Instructions, ()> for Admission {
    type Refusal = ();

    fn validate_description(_: &MachineDescription<'_, Instructions, ()>) -> Result<(), ()> {
        Ok(())
    }

    fn validate_instruction(_: &Instruction, _: &()) -> Result<(), ()> {
        Ok(())
    }

    fn encode_metadata(_: &(), _: &mut Vec<u8>) {}

    fn encode_instruction(instruction: &Instruction, output: &mut Vec<u8>) {
        output.push(instruction.id);
        match instruction.operation {
            Operation::Constant(value) => output.extend_from_slice(&value.to_le_bytes()),
            Operation::Divide => output.push(1),
            Operation::Add => output.push(2),
            Operation::Call(target) => output.extend_from_slice(&[3, target]),
            Operation::Interrupt => output.push(4),
            Operation::Return => output.push(5),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Value {
    number: i64,
    root: Option<ManagedId>,
}

struct Values;

impl ValueWidthPolicy for Values {
    type Value = Value;

    fn width(_: &Value) -> usize {
        1
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Abrupt {
    DivisionByZero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Interrupt {
    Poll,
}

struct StackFrame {
    cursor: sim_lib_machine::CodeCursor,
    operands: UnitStack<Values>,
}

impl MachineFrame for StackFrame {
    fn cursor(&self) -> sim_lib_machine::CodeCursor {
        self.cursor
    }

    fn set_cursor(&mut self, cursor: sim_lib_machine::CodeCursor) {
        self.cursor = cursor;
    }
}

impl ManagedRootSource for StackFrame {
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        let mut complete = true;
        self.operands.visit_values(|value| {
            if complete && let Some(root) = value.root {
                complete = visit(root);
            }
        });
        complete
    }
}

struct StackCalculator<'a> {
    code: &'a LocatedCode<Instructions>,
    interrupt_once: bool,
}

impl InstructionDriverPolicy<Instructions, StackFrame> for StackCalculator<'_> {
    type Return = ();
    type Abrupt = Abrupt;
    type Yield = ();
    type Interrupt = Interrupt;
    type Fault = ();

    fn step(
        &mut self,
        instruction: &Instruction,
        frame: &mut StackFrame,
    ) -> Result<StepOutcome<StackFrame, (), Abrupt, (), Interrupt>, ()> {
        let next = self.code.next(frame.cursor());
        match instruction.operation {
            Operation::Constant(number) => {
                frame.operands.push(Value { number, root: None }).unwrap();
                Ok(StepOutcome::Continue(
                    next.expect("nonterminal instruction"),
                ))
            }
            Operation::Divide => {
                let divisor = frame.operands.pop().unwrap();
                if divisor.number == 0 {
                    Ok(StepOutcome::Raise(Abrupt::DivisionByZero))
                } else {
                    let dividend = frame.operands.pop().unwrap();
                    frame
                        .operands
                        .push(Value {
                            number: dividend.number / divisor.number,
                            root: dividend.root,
                        })
                        .unwrap();
                    Ok(StepOutcome::Continue(
                        next.expect("nonterminal instruction"),
                    ))
                }
            }
            Operation::Add => {
                let right = frame.operands.pop().unwrap();
                let left = frame.operands.pop().unwrap();
                frame
                    .operands
                    .push(Value {
                        number: left.number + right.number,
                        root: left.root.or(right.root),
                    })
                    .unwrap();
                Ok(StepOutcome::Continue(
                    next.expect("nonterminal instruction"),
                ))
            }
            Operation::Call(target) => {
                frame.set_cursor(next.expect("nonterminal instruction"));
                Ok(StepOutcome::Call(stack_frame(
                    self.code.cursor(target).unwrap(),
                )))
            }
            Operation::Interrupt if self.interrupt_once => {
                self.interrupt_once = false;
                frame.set_cursor(next.expect("nonterminal instruction"));
                Ok(StepOutcome::Interrupt(Interrupt::Poll))
            }
            Operation::Interrupt => Ok(StepOutcome::Continue(
                next.expect("nonterminal instruction"),
            )),
            Operation::Return => Ok(StepOutcome::Return(())),
        }
    }
}

#[test]
fn stack_calculator_protects_division_and_resumes_exactly() {
    let code = specimen_code();
    let description = description(&code);
    let permit = permit(&description);
    let root = managed_root();
    let mut frames = FrameStack::new(WorkLimit(2));
    let mut outer = stack_frame(code.entry());
    outer
        .operands
        .push(Value {
            number: 12,
            root: Some(root),
        })
        .unwrap();
    frames.push(outer).unwrap();
    let mut driver = Driver::new(StackCalculator {
        code: &code,
        interrupt_once: true,
    });

    let prefix = match driver
        .drive_protected::<_, _, Admission, _>(&description, &permit, &mut frames, WorkLimit(16))
        .unwrap()
    {
        DriveOutcome::Interrupt(Interrupt::Poll, receipt) => receipt,
        _ => panic!("stack specimen must interrupt"),
    };
    assert_eq!(
        prefix.steps(),
        &[
            (1, StepKind::Continue),
            (2, StepKind::Continue),
            (3, StepKind::Raise),
            (4, StepKind::Continue),
            (5, StepKind::Call),
            (9, StepKind::Continue),
            (10, StepKind::Return),
            (6, StepKind::Interrupt)
        ]
    );
    assert_eq!(
        RootSnapshot::scan(&frames, WorkLimit(1)).unwrap().roots(),
        &[root]
    );

    let checkpoint = MachineCheckpoint::new(frames, &permit, prefix.clone());
    assert_eq!(checkpoint.evidence().receipt(), &prefix);
    let Ok((mut frames, prefix)) = checkpoint.resume(&permit) else {
        panic!("the original permit must resume the stack specimen")
    };
    let tail = match driver
        .drive_protected::<_, _, Admission, _>(&description, &permit, &mut frames, WorkLimit(4))
        .unwrap()
    {
        DriveOutcome::Return((), receipt) => receipt,
        _ => panic!("resumed stack specimen must return"),
    };
    assert_eq!(
        tail.steps(),
        &[(7, StepKind::Continue), (8, StepKind::Return)]
    );
    assert_eq!(prefix.charged() + tail.charged(), 10);
    assert_eq!(frames.depth(), 0);
}

struct RegisterFrame {
    cursor: sim_lib_machine::CodeCursor,
    registers: SlotFile<Values>,
}

impl MachineFrame for RegisterFrame {
    fn cursor(&self) -> sim_lib_machine::CodeCursor {
        self.cursor
    }
    fn set_cursor(&mut self, cursor: sim_lib_machine::CodeCursor) {
        self.cursor = cursor;
    }
}

impl ManagedRootSource for RegisterFrame {
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        let mut complete = true;
        self.registers.visit_values(|value| {
            if complete && let Some(root) = value.root {
                complete = visit(root);
            }
        });
        complete
    }
}

struct RegisterMachine<'a> {
    code: &'a LocatedCode<Instructions>,
    interrupt_once: bool,
}

impl InstructionDriverPolicy<Instructions, RegisterFrame> for RegisterMachine<'_> {
    type Return = ();
    type Abrupt = Abrupt;
    type Yield = ();
    type Interrupt = Interrupt;
    type Fault = ();

    fn step(
        &mut self,
        instruction: &Instruction,
        frame: &mut RegisterFrame,
    ) -> Result<StepOutcome<RegisterFrame, (), Abrupt, (), Interrupt>, ()> {
        let next = self.code.next(frame.cursor());
        match instruction.operation {
            Operation::Constant(number) => {
                let slot = if instruction.id == 1 { 0 } else { 1 };
                let root = frame.registers.load(slot).ok().and_then(|value| value.root);
                frame.registers.store(slot, Value { number, root }).unwrap();
                Ok(StepOutcome::Continue(
                    next.expect("nonterminal instruction"),
                ))
            }
            Operation::Divide => {
                let divisor = *frame.registers.load(1).unwrap();
                if divisor.number == 0 {
                    Ok(StepOutcome::Raise(Abrupt::DivisionByZero))
                } else {
                    let dividend = *frame.registers.load(0).unwrap();
                    frame
                        .registers
                        .store(
                            0,
                            Value {
                                number: dividend.number / divisor.number,
                                root: dividend.root,
                            },
                        )
                        .unwrap();
                    Ok(StepOutcome::Continue(
                        next.expect("nonterminal instruction"),
                    ))
                }
            }
            Operation::Add => {
                let left = *frame.registers.load(0).unwrap();
                let right = *frame.registers.load(1).unwrap();
                frame
                    .registers
                    .store(
                        0,
                        Value {
                            number: left.number + right.number,
                            root: left.root.or(right.root),
                        },
                    )
                    .unwrap();
                Ok(StepOutcome::Continue(
                    next.expect("nonterminal instruction"),
                ))
            }
            Operation::Call(target) => {
                frame.set_cursor(next.expect("nonterminal instruction"));
                Ok(StepOutcome::Call(register_frame(
                    self.code.cursor(target).unwrap(),
                )))
            }
            Operation::Interrupt if self.interrupt_once => {
                self.interrupt_once = false;
                frame.set_cursor(next.expect("nonterminal instruction"));
                Ok(StepOutcome::Interrupt(Interrupt::Poll))
            }
            Operation::Interrupt => Ok(StepOutcome::Continue(
                next.expect("nonterminal instruction"),
            )),
            Operation::Return => Ok(StepOutcome::Return(())),
        }
    }
}

#[test]
fn register_machine_tracks_initialization_without_an_operand_stack() {
    let code = specimen_code();
    let description = description(&code);
    let permit = permit(&description);
    let root = managed_root();
    let mut outer = register_frame(code.entry());
    assert_eq!(
        outer.registers.load(0),
        Err(SlotError::Uninitialized { slot: 0 })
    );
    outer
        .registers
        .store(
            0,
            Value {
                number: 12,
                root: Some(root),
            },
        )
        .unwrap();
    let mut frames = FrameStack::new(WorkLimit(2));
    frames.push(outer).unwrap();
    let mut driver = Driver::new(RegisterMachine {
        code: &code,
        interrupt_once: true,
    });

    let prefix = match driver
        .drive_protected::<_, _, Admission, _>(&description, &permit, &mut frames, WorkLimit(16))
        .unwrap()
    {
        DriveOutcome::Interrupt(Interrupt::Poll, receipt) => receipt,
        _ => panic!("register specimen must interrupt"),
    };
    assert_eq!(prefix.steps()[2], (3, StepKind::Raise));
    assert_eq!(prefix.steps()[4], (5, StepKind::Call));
    assert_eq!(
        RootSnapshot::scan(&frames, WorkLimit(1)).unwrap().roots(),
        &[root]
    );

    let checkpoint = MachineCheckpoint::new(frames, &permit, prefix.clone());
    let Ok((mut frames, recorded_prefix)) = checkpoint.resume(&permit) else {
        panic!("the original permit must resume the register specimen")
    };
    assert_eq!(recorded_prefix, prefix);
    let tail = match driver
        .drive_protected::<_, _, Admission, _>(&description, &permit, &mut frames, WorkLimit(4))
        .unwrap()
    {
        DriveOutcome::Return((), receipt) => receipt,
        _ => panic!("resumed register specimen must return"),
    };
    assert_eq!(
        tail.steps(),
        &[(7, StepKind::Continue), (8, StepKind::Return)]
    );
    assert_eq!(recorded_prefix.charged() + tail.charged(), 10);
}

fn specimen_code() -> LocatedCode<Instructions> {
    let operations = [
        Operation::Constant(12),
        Operation::Constant(0),
        Operation::Divide,
        Operation::Constant(3),
        Operation::Call(9),
        Operation::Interrupt,
        Operation::Add,
        Operation::Return,
        Operation::Constant(2),
        Operation::Return,
    ];
    LocatedCode::freeze(
        operations
            .into_iter()
            .enumerate()
            .map(|(index, operation)| {
                let id = u8::try_from(index + 1).unwrap();
                LocatedInstruction::new(
                    Instruction { id, operation },
                    id,
                    SourceLocation::Bytes(Origin {
                        codec: CodecId(1),
                        source: SourceId("neutral-machine-specimen".into()),
                        span: Span {
                            start: index,
                            end: index + 1,
                        },
                        trivia: vec![],
                    }),
                    matches!(operation, Operation::Call(_) | Operation::Interrupt),
                    None,
                )
            })
            .collect(),
        vec![],
        vec![RegionSpec {
            start: 3,
            end: Some(4),
            handler: TargetLocation::Instruction(4),
        }],
    )
    .unwrap()
}

fn description(code: &LocatedCode<Instructions>) -> MachineDescription<'_, Instructions, ()> {
    MachineDescription::new(
        code,
        AdmissionLimits {
            instructions: 10,
            operand_units: 4,
            slots: 4,
            frames: 2,
            work: 16,
        },
        &(),
    )
}

fn permit(description: &MachineDescription<'_, Instructions, ()>) -> MachinePermit {
    MachinePermit::admit::<_, _, Admission>(description).unwrap()
}

fn stack_frame(cursor: sim_lib_machine::CodeCursor) -> StackFrame {
    StackFrame {
        cursor,
        operands: UnitStack::new(WorkLimit(4)),
    }
}

fn register_frame(cursor: sim_lib_machine::CodeCursor) -> RegisterFrame {
    RegisterFrame {
        cursor,
        registers: SlotFile::new(AdmissionLimit(4)),
    }
}

fn managed_root() -> ManagedId {
    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(1).unwrap());
    arena.allocate(ManagedNode::new(())).unwrap().id()
}

#[test]
fn specimens_import_only_neutral_machine_vocabulary() {
    let source = include_str!("neutral_machine_specimens.rs").to_ascii_lowercase();
    for forbidden in [
        concat!("j", "vm"),
        concat!("class", "file"),
        concat!("guest", "-language"),
        concat!("guest", "_language"),
    ] {
        assert!(
            !source.contains(forbidden),
            "forbidden type import: {forbidden}"
        );
    }
    let register_storage = source
        .split("struct registerframe")
        .nth(1)
        .unwrap()
        .split("struct registermachine")
        .next()
        .unwrap();
    assert!(!register_storage.contains(concat!("unit", "stack")));
}
