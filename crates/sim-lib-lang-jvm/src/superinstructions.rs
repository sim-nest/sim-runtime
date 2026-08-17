//! Generated, boundary-preserving straight-line fusion plans.

use sim_codec_classfile::{InstructionId, Opcode};
use sim_lib_machine::{CodeCursor, LocatedCode, SourceLocation};

use crate::verifier::PreparedDispatchFamily;
use crate::{PreparedJvmPolicy, PreparedMicroOp, RootEffect};

include!("superinstructions_generated.rs");

/// One generated fused region with exact maps back to its unfused instructions.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedFusion {
    handler: FusedHandler,
    expansion: Box<[InstructionId]>,
    sources: Box<[SourceLocation]>,
    work: Box<[usize]>,
    roots: Box<[RootEffect]>,
}

impl PreparedFusion {
    /// Returns the generated handler selected for this region.
    pub const fn handler(&self) -> FusedHandler {
        self.handler
    }
    /// Expands the region to the exact original instruction identities.
    pub fn expansion(&self) -> &[InstructionId] {
        &self.expansion
    }
    /// Returns the instruction-by-instruction source map.
    pub fn sources(&self) -> &[SourceLocation] {
        &self.sources
    }
    /// Returns the instruction-by-instruction work-charge map.
    pub fn work_map(&self) -> &[usize] {
        &self.work
    }
    /// Returns the instruction-by-instruction managed-root map.
    pub fn root_map(&self) -> &[RootEffect] {
        &self.roots
    }
}

/// Deterministically selects non-overlapping generated fusions in source order.
pub fn prepare_fusions(code: &LocatedCode<PreparedJvmPolicy>) -> Box<[PreparedFusion]> {
    let mut fusions = Vec::new();
    let mut at = 0;
    while at < code.len() {
        if let Some(definition) = FUSED_DEFINITIONS
            .iter()
            .find(|definition| eligible(code, at, definition))
        {
            let range = at..at + definition.opcodes.len();
            fusions.push(PreparedFusion {
                handler: definition.handler,
                expansion: range
                    .clone()
                    .map(|index| InstructionId(index as u32))
                    .collect(),
                sources: range
                    .clone()
                    .map(|index| code.instruction(cursor(code, index)).location().clone())
                    .collect(),
                work: range
                    .clone()
                    .map(|index| {
                        code.instruction(cursor(code, index))
                            .instruction()
                            .work_charge()
                    })
                    .collect(),
                roots: range
                    .map(|index| {
                        code.instruction(cursor(code, index))
                            .instruction()
                            .root_effect()
                    })
                    .collect(),
            });
            at += definition.opcodes.len();
        } else {
            at += 1;
        }
    }
    fusions.into_boxed_slice()
}

fn eligible(
    code: &LocatedCode<PreparedJvmPolicy>,
    start: usize,
    definition: &FusedDefinition,
) -> bool {
    start + definition.opcodes.len() <= code.len()
        && (0..definition.opcodes.len()).all(|offset| {
            let located = code.instruction(cursor(code, start + offset));
            let instruction = located.instruction();
            instruction.opcode() == definition.opcodes[offset]
                && !located.is_safepoint()
                && instruction.dispatch_family() != PreparedDispatchFamily::Control
                && matches!(instruction.micro_op(), PreparedMicroOp::Checked)
                && instruction.handler_membership().is_empty()
                && instruction.handler_entries().is_empty()
                && code.branch_targets(*located.id()).is_empty()
                && (offset == 0 || !is_branch_destination(code, cursor(code, start + offset)))
                && !is_excluded_effect(instruction.opcode())
        })
}

fn is_branch_destination(code: &LocatedCode<PreparedJvmPolicy>, destination: CodeCursor) -> bool {
    (0..code.len()).any(|index| {
        let from = code.instruction(cursor(code, index));
        code.branch_targets(*from.id()).contains(&destination)
    })
}

fn is_excluded_effect(opcode: Opcode) -> bool {
    let mnemonic = opcode.metadata().mnemonic;
    mnemonic.starts_with("invoke")
        || mnemonic.starts_with("new")
        || mnemonic.contains("field")
        || mnemonic.contains("array")
        || matches!(mnemonic, "athrow" | "monitorenter" | "monitorexit")
}

fn cursor(code: &LocatedCode<PreparedJvmPolicy>, index: usize) -> CodeCursor {
    code.cursor(InstructionId(index as u32))
        .expect("prepared JVM identities are dense")
}
