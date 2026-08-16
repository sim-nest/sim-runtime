//! Prepared work regions that preserve exact instruction and safepoint observations.

use sim_codec_classfile::InstructionId;
use sim_lib_machine::{CodeCursor, LocatedCode};

use crate::PreparedJvmPolicy;
use crate::verifier::PreparedDispatchFamily;

/// One maximal straight-line region whose work can be admitted as a batch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedWorkRegion {
    start: InstructionId,
    instruction_count: usize,
    aggregate_charge: usize,
}

impl PreparedWorkRegion {
    /// Returns the first instruction in the region.
    pub const fn start(self) -> InstructionId {
        self.start
    }

    /// Returns the number of instructions in the region.
    pub const fn instruction_count(self) -> usize {
        self.instruction_count
    }

    /// Returns the precomputed sum of the region's instruction charges.
    pub const fn aggregate_charge(self) -> usize {
        self.aggregate_charge
    }
}

/// Immutable work-region index derived from prepared JVM code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedWorkRegions {
    regions: Box<[PreparedWorkRegion]>,
    instruction_regions: Box<[usize]>,
}

impl PreparedWorkRegions {
    /// Partitions prepared code at safepoints, control transfers, and handler boundaries.
    pub fn prepare(code: &LocatedCode<PreparedJvmPolicy>) -> Self {
        let mut regions = Vec::new();
        let mut instruction_regions = vec![0; code.len()];
        let mut start = 0;
        while start < code.len() {
            let mut end = start + 1;
            let mut charge = code
                .instruction(cursor(code, start))
                .instruction()
                .work_charge();
            while end < code.len() && may_share_region(code, end - 1, end) {
                charge = charge
                    .checked_add(
                        code.instruction(cursor(code, end))
                            .instruction()
                            .work_charge(),
                    )
                    .expect("prepared JVM region work charge overflow");
                end += 1;
            }
            let region_index = regions.len();
            instruction_regions[start..end].fill(region_index);
            regions.push(PreparedWorkRegion {
                start: InstructionId(start as u32),
                instruction_count: end - start,
                aggregate_charge: charge,
            });
            start = end;
        }
        Self {
            regions: regions.into_boxed_slice(),
            instruction_regions: instruction_regions.into_boxed_slice(),
        }
    }

    /// Returns the prepared regions in instruction order.
    pub fn regions(&self) -> &[PreparedWorkRegion] {
        &self.regions
    }

    /// Chooses an execution window without moving an observable stop location.
    ///
    /// A complete region is admitted only at its first instruction, when no interruption is
    /// pending and its aggregate charge fits. Every other case deliberately falls back to one
    /// instruction, so the caller observes exhaustion and interruption exactly as in baseline
    /// stepping mode.
    pub fn execution_window(
        &self,
        code: &LocatedCode<PreparedJvmPolicy>,
        at: CodeCursor,
        remaining_work: usize,
        interruption_pending: bool,
    ) -> usize {
        let index = code.instruction(at).instruction().id().0 as usize;
        let region = self.regions[self.instruction_regions[index]];
        if region.start.0 as usize == index
            && !interruption_pending
            && region.aggregate_charge <= remaining_work
        {
            region.instruction_count
        } else {
            1
        }
    }
}

fn may_share_region(code: &LocatedCode<PreparedJvmPolicy>, left: usize, right: usize) -> bool {
    let left_cursor = cursor(code, left);
    let right_cursor = cursor(code, right);
    let left_instruction = code.instruction(left_cursor);
    let right_instruction = code.instruction(right_cursor);
    !right_instruction.is_safepoint()
        && left_instruction.instruction().dispatch_family() != PreparedDispatchFamily::Control
        && code.branch_targets(*left_instruction.id()).is_empty()
        && left_instruction.instruction().handler_membership()
            == right_instruction.instruction().handler_membership()
        && left_instruction.instruction().handler_entries().is_empty()
        && right_instruction.instruction().handler_entries().is_empty()
}

fn cursor(code: &LocatedCode<PreparedJvmPolicy>, index: usize) -> CodeCursor {
    code.cursor(InstructionId(index as u32))
        .expect("prepared JVM instructions retain dense identities")
}
