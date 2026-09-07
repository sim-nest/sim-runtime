//! The single production policy connecting prepared JVM code to the neutral machine.

use sim_codec_classfile::{InstructionId, Opcode};
use sim_kernel::{ContentId, Datum, Symbol};
use sim_lib_control::WorkLimit;
use sim_lib_machine::{
    AdmissionLimits, AdmissionPolicy, CodeCursor, DriveOutcome, Driver, FrameStack,
    InstructionDriverPolicy, MachineDescription, MachineFrame, MachinePermit, ManagedRootSource,
    StepOutcome,
};
use sim_lib_mutation::ManagedId;
use std::sync::Mutex;

use crate::{
    ExecutionError, ExecutionPermit, JvmControlOutcome, JvmFrameLease, JvmInstructionPolicy,
    JvmInstructionSemantics, JvmInvocationError, JvmSlotKind, JvmValue, NumericExecutionError,
    PreparedDispatch, PreparedJvmInstruction, PreparedJvmPolicy, VerificationFidelity,
    dispatch_prepared, execute_control_instruction, execute_numeric_instruction,
    execute_storage_instruction,
};

impl From<sim_kernel::Error> for JvmInvocationError {
    fn from(error: sim_kernel::Error) -> Self {
        Self::Admission(error.to_string())
    }
}

/// Preparation disposition for one row of the shared generated opcode inventory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SurfaceOpcodeDisposition {
    /// The production driver has an instruction policy and reachable family handler.
    Handler(crate::verifier::PreparedDispatchFamily),
    /// The production surface refuses the row during effect-free preparation.
    PreparationRefusal,
}

/// Classifies a shared opcode row without duplicating its identity or mnemonic.
pub fn surface_opcode_disposition(opcode: Opcode) -> SurfaceOpcodeDisposition {
    match (
        crate::verifier::PREPARED_DISPATCH[opcode as u8 as usize],
        SurfacePolicy::semantics(opcode),
    ) {
        (Some(family), Some(_)) => SurfaceOpcodeDisposition::Handler(family),
        _ => SurfaceOpcodeDisposition::PreparationRefusal,
    }
}

const NONE: &[JvmSlotKind] = &[];
const ONE: &[JvmSlotKind] = &[JvmSlotKind::CategoryOne];
const TWO: &[JvmSlotKind] = &[JvmSlotKind::CategoryOne, JvmSlotKind::CategoryOne];

/// Evidence that exact class bytes were prepared for a method drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmPreparationReceipt {
    /// Identity of the admitted prepared machine description.
    pub content: ContentId,
    /// Fidelity established before preparation.
    pub fidelity: VerificationFidelity,
    /// Number of decoded instructions frozen into prepared code.
    pub instructions: usize,
}

/// Evidence for one complete prepared drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmDriveReceipt {
    /// Exact charged prepared instruction identities.
    pub work: Vec<InstructionId>,
    /// Number of complete root publications at prepared safepoints.
    pub safepoints: usize,
    /// Total roots observed across those publications.
    pub roots: usize,
    /// Whether terminal lease cleanup ran.
    pub cleaned_up: bool,
}

pub(crate) struct SurfacePolicy;

impl JvmInstructionPolicy for SurfacePolicy {
    fn semantics(opcode: Opcode) -> Option<JvmInstructionSemantics> {
        use Opcode::*;
        let (pops, pushes) = match opcode {
            Iload | Iload0 | Iload1 | Iload2 | Iload3 => (NONE, ONE),
            Istore | Istore0 | Istore1 | Istore2 | Istore3 => (ONE, NONE),
            Iinc => (NONE, NONE),
            IconstM1 | Iconst0 | Iconst1 | Iconst2 | Iconst3 | Iconst4 | Iconst5 | Bipush
            | Sipush => (NONE, ONE),
            Iadd | Isub | Imul | Idiv | Irem => (TWO, ONE),
            Ineg | I2b | I2c | I2s => (ONE, ONE),
            Ireturn => (ONE, NONE),
            Return | Goto | GotoW => (NONE, NONE),
            Ifeq | Ifne | Iflt | Ifge | Ifgt | Ifle => (ONE, NONE),
            IfIcmpeq | IfIcmpne | IfIcmplt | IfIcmpge | IfIcmpgt | IfIcmple => (TWO, NONE),
            _ => return None,
        };
        Some(JvmInstructionSemantics {
            pops,
            pushes,
            safepoint: false,
        })
    }
}

pub(crate) struct SurfaceAdmission;

impl AdmissionPolicy<PreparedJvmPolicy, ()> for SurfaceAdmission {
    type Refusal = ();
    fn validate_description(_: &MachineDescription<'_, PreparedJvmPolicy, ()>) -> Result<(), ()> {
        Ok(())
    }
    fn validate_instruction(_: &PreparedJvmInstruction, _: &()) -> Result<(), ()> {
        Ok(())
    }
    fn policy_identity() -> Symbol {
        Symbol::qualified("jvm", "surface-admission")
    }
    fn policy_version() -> u32 {
        1
    }
    fn metadata_datum(_: &()) -> Datum {
        Datum::Nil
    }
    fn instruction_datum(instruction: &PreparedJvmInstruction) -> Datum {
        instruction.semantic_datum()
    }
}

struct DriveFrame {
    lease: Option<JvmFrameLease>,
    cursor: CodeCursor,
}

impl MachineFrame for DriveFrame {
    fn cursor(&self) -> CodeCursor {
        self.cursor
    }
    fn set_cursor(&mut self, cursor: CodeCursor) {
        self.cursor = cursor;
    }
}

impl ManagedRootSource for DriveFrame {
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        self.lease
            .as_ref()
            .expect("live JVM drive lease")
            .visit_managed_roots(visit)
    }
}

type PolicyOutcome = StepOutcome<DriveFrame, Option<JvmValue>, Box<crate::JavaThrowable>, (), ()>;

struct ProductionPolicy<'a> {
    code: &'a sim_lib_machine::LocatedCode<PreparedJvmPolicy>,
}

impl ProductionPolicy<'_> {
    fn dispatch(
        &mut self,
        instruction: &PreparedJvmInstruction,
        frame: &mut DriveFrame,
    ) -> Result<PolicyOutcome, JvmInvocationError> {
        struct Bound<'a, 'b> {
            owner: &'a mut ProductionPolicy<'b>,
            frame: &'a mut DriveFrame,
        }
        impl PreparedDispatch for Bound<'_, '_> {
            type Output = Result<PolicyOutcome, JvmInvocationError>;
            fn storage(&mut self, instruction: &PreparedJvmInstruction) -> Self::Output {
                let record = self.frame.lease.as_mut().expect("live lease").frame_mut();
                let (locals, operands) = record.execution_storage_mut();
                execute_storage_instruction(instruction, locals, operands, &mut |index| {
                    Err(ExecutionError::Constant { index })
                })
                .map_err(|e| JvmInvocationError::Resource(format!("prepared storage: {e:?}")))?;
                Ok(StepOutcome::Continue(
                    self.owner.code.next(self.frame.cursor).ok_or_else(|| {
                        JvmInvocationError::Admission(
                            "prepared storage instruction has no fallthrough".into(),
                        )
                    })?,
                ))
            }
            fn numeric(&mut self, instruction: &PreparedJvmInstruction) -> Self::Output {
                let record = self.frame.lease.as_mut().expect("live lease").frame_mut();
                execute_numeric_instruction(instruction, record.operands_mut(), || {
                    panic!("arithmetic throwable allocation is outside this integer profile")
                })
                .map_err(|e: NumericExecutionError| {
                    JvmInvocationError::Resource(format!("prepared numeric: {e:?}"))
                })?;
                Ok(StepOutcome::Continue(
                    self.owner.code.next(self.frame.cursor).ok_or_else(|| {
                        JvmInvocationError::Admission(
                            "prepared numeric instruction has no fallthrough".into(),
                        )
                    })?,
                ))
            }
            fn control(&mut self, _: &PreparedJvmInstruction) -> Self::Output {
                let record = self.frame.lease.as_mut().expect("live lease").frame_mut();
                match execute_control_instruction(
                    self.owner.code,
                    self.frame.cursor,
                    record.operands_mut(),
                    false,
                )
                .map_err(|e| JvmInvocationError::Resource(format!("prepared control: {e:?}")))?
                {
                    JvmControlOutcome::Continue { cursor, .. } => Ok(StepOutcome::Continue(cursor)),
                    JvmControlOutcome::Return { value, .. } => {
                        self.frame
                            .lease
                            .take()
                            .expect("live terminal lease")
                            .complete();
                        Ok(StepOutcome::Return(value))
                    }
                    JvmControlOutcome::Interrupted { .. } => Ok(StepOutcome::Interrupt(())),
                }
            }
            fn object(&mut self, instruction: &PreparedJvmInstruction) -> Self::Output {
                Err(JvmInvocationError::Admission(format!(
                    "prepared object opcode {:?} is not installed",
                    instruction.opcode()
                )))
            }
        }
        dispatch_prepared(instruction, &mut Bound { owner: self, frame })
    }
}

impl InstructionDriverPolicy<PreparedJvmPolicy, DriveFrame> for ProductionPolicy<'_> {
    type Return = Option<JvmValue>;
    type Abrupt = Box<crate::JavaThrowable>;
    type Yield = ();
    type Interrupt = ();
    type Fault = JvmInvocationError;
    fn step(
        &mut self,
        instruction: &PreparedJvmInstruction,
        frame: &mut DriveFrame,
    ) -> Result<PolicyOutcome, Self::Fault> {
        self.dispatch(instruction, frame)
    }
}

pub(crate) fn drive_i32<P>(
    permit: ExecutionPermit<'_, P>,
    code: &sim_lib_machine::LocatedCode<PreparedJvmPolicy>,
    machine: &MachinePermit,
    limits: AdmissionLimits,
    lease: JvmFrameLease,
    heap: &Mutex<crate::JvmHeap>,
) -> Result<(i32, JvmPreparationReceipt, JvmDriveReceipt), JvmInvocationError> {
    permit
        .validate_current()
        .map_err(|e| JvmInvocationError::Admission(e.to_string()))?;
    if permit.machine_content() != machine.content_id() {
        return Err(JvmInvocationError::Admission(
            "execution permit does not bind this prepared method".into(),
        ));
    }
    let description = MachineDescription::new(code, limits, &());
    let mut frames = FrameStack::new(WorkLimit(limits.frames));
    frames
        .push(DriveFrame {
            lease: Some(lease),
            cursor: code.entry(),
        })
        .map_err(|e| JvmInvocationError::Resource(format!("frame admission: {e:?}")))?;
    let mut driver = Driver::new(ProductionPolicy { code });
    let mut safepoints = 0;
    let mut roots = 0;
    let outcome = match driver
        .drive_with_safepoints::<PreparedJvmPolicy, _, SurfaceAdmission, _, _>(
            &description,
            machine,
            &mut frames,
            WorkLimit(limits.work),
            WorkLimit(limits.slots + limits.operand_units),
            |snapshot| {
                safepoints += 1;
                roots += snapshot.roots().len();
                heap.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .collect_from(snapshot)
                    .map(|_| ())
                    .map_err(|_| ())
            },
        ) {
        Ok(outcome) => outcome,
        Err(error) => {
            interrupt_live_frame(&mut frames);
            return Err(JvmInvocationError::Resource(format!(
                "machine drive: {error:?}"
            )));
        }
    };
    let (value, work) = match outcome {
        DriveOutcome::Return(Some(JvmValue::Int(value)), receipt) => (value, receipt),
        DriveOutcome::Continue(_) => {
            interrupt_live_frame(&mut frames);
            return Err(JvmInvocationError::Resource(
                "machine work budget exhausted".into(),
            ));
        }
        DriveOutcome::Return(_, _) => {
            return Err(JvmInvocationError::Admission(
                "prepared method returned a non-int value".into(),
            ));
        }
        DriveOutcome::Raise(value, _) => {
            interrupt_live_frame(&mut frames);
            return Err(JvmInvocationError::JavaThrowable(value));
        }
        DriveOutcome::Yield(_, _) | DriveOutcome::Interrupt(_, _) => {
            interrupt_live_frame(&mut frames);
            return Err(JvmInvocationError::Resource(
                "prepared drive interrupted".into(),
            ));
        }
    };
    Ok((
        value,
        JvmPreparationReceipt {
            content: machine.content_id().clone(),
            fidelity: permit.fidelity(),
            instructions: code.len(),
        },
        JvmDriveReceipt {
            work: work.steps().iter().map(|(id, _)| *id).collect(),
            safepoints,
            roots,
            cleaned_up: true,
        },
    ))
}

fn interrupt_live_frame(frames: &mut FrameStack<DriveFrame>) {
    if let Some(mut frame) = frames.pop()
        && let Some(lease) = frame.lease.take()
    {
        drop(lease.interrupt());
    }
}

#[cfg(test)]
mod tests {
    use sim_codec_classfile::OPCODES;

    use super::*;

    #[test]
    fn every_shared_opcode_row_has_a_driver_handler_or_preparation_refusal() {
        let mut handlers = 0;
        let mut refusals = 0;
        for (byte, metadata) in OPCODES.iter().enumerate() {
            assert_eq!(metadata.opcode as u8 as usize, byte);
            match surface_opcode_disposition(metadata.opcode) {
                SurfaceOpcodeDisposition::Handler(_) => handlers += 1,
                SurfaceOpcodeDisposition::PreparationRefusal => refusals += 1,
            }
        }
        assert_eq!(handlers + refusals, 256);
        assert!(handlers > 0);
        assert!(refusals > 0);
    }
}
