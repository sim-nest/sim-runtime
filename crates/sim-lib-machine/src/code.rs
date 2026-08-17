use std::collections::BTreeMap;

use sha2::{Digest, Sha256};
use sim_kernel::{Origin, SourceId};

use crate::InstructionPolicy;

/// Source units used to locate a decoded instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceLocation {
    /// A byte range described by the kernel's lossless source contract.
    Bytes(Origin),
    /// A half-open token range, with the enclosing byte origin retained for diagnostics.
    Tokens {
        /// The enclosing source origin.
        origin: Origin,
        /// Inclusive first token index.
        start: usize,
        /// Exclusive token index.
        end: usize,
    },
}

impl SourceLocation {
    fn source(&self) -> &SourceId {
        match self {
            Self::Bytes(origin) | Self::Tokens { origin, .. } => &origin.source,
        }
    }

    fn range(&self) -> (LocationUnit, usize, usize) {
        match self {
            Self::Bytes(origin) => (LocationUnit::Byte, origin.span.start, origin.span.end),
            Self::Tokens { start, end, .. } => (LocationUnit::Token, *start, *end),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LocationUnit {
    Byte,
    Token,
}

/// Stable metadata used to associate execution with a coverage counter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoverageMetadata {
    /// Stable counter identity assigned by the preparing consumer.
    pub counter: u64,
}

/// A decoded instruction and all immutable metadata required before execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocatedInstruction<I, Id> {
    instruction: I,
    id: Id,
    location: SourceLocation,
    safepoint: bool,
    coverage: Option<CoverageMetadata>,
}

impl<I, Id> LocatedInstruction<I, Id> {
    /// Prepares one instruction. Its identity is checked against the policy while code is frozen.
    pub fn new(
        instruction: I,
        id: Id,
        location: SourceLocation,
        safepoint: bool,
        coverage: Option<CoverageMetadata>,
    ) -> Self {
        Self {
            instruction,
            id,
            location,
            safepoint,
            coverage,
        }
    }

    /// Returns the decoded instruction.
    pub fn instruction(&self) -> &I {
        &self.instruction
    }

    /// Returns its stable identity.
    pub fn id(&self) -> &Id {
        &self.id
    }

    /// Returns its source location.
    pub fn location(&self) -> &SourceLocation {
        &self.location
    }

    /// Returns whether this is a semantic safepoint.
    pub fn is_safepoint(&self) -> bool {
        self.safepoint
    }

    /// Returns the optional stable coverage-counter metadata.
    pub fn coverage(&self) -> Option<CoverageMetadata> {
        self.coverage
    }
}

/// An unresolved branch destination supplied by a code preparer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TargetLocation<Id> {
    /// A stable instruction identity.
    Instruction(Id),
    /// An exact byte boundary in a kernel-identified source.
    Byte {
        /// Source containing the destination.
        source: SourceId,
        /// Requested byte offset.
        offset: usize,
    },
    /// An exact token boundary in a kernel-identified source.
    Token {
        /// Source containing the destination.
        source: SourceId,
        /// Requested token index.
        index: usize,
    },
}

/// A branch edge to validate and freeze into the target map.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchTarget<Id> {
    /// Identity of the instruction containing the branch.
    pub from: Id,
    /// Requested destination.
    pub to: TargetLocation<Id>,
}

/// A protected-region declaration using half-open instruction identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionSpec<Id> {
    /// First protected instruction.
    pub start: Id,
    /// First instruction after the protected range, or `None` for code end.
    pub end: Option<Id>,
    /// Handler entry, which must resolve to an instruction boundary.
    pub handler: TargetLocation<Id>,
}

/// A validated protected region whose positions can only be valid cursors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtectedRegion {
    /// First protected instruction.
    pub start: CodeCursor,
    /// First instruction after the region; equal to `instruction_count` at code end.
    pub end_index: usize,
    /// Validated handler entry.
    pub handler: CodeCursor,
}

/// An instruction position minted only by validated [`LocatedCode`].
///
/// Raw offsets cannot create cursors:
///
/// ```compile_fail
/// use sim_lib_machine::CodeCursor;
/// let cursor = CodeCursor(7);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CodeCursor(usize);

/// Exact refusal evidence produced while freezing located code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CodeError<Id> {
    /// No instructions were supplied.
    Empty,
    /// An instruction's location is empty or reversed.
    MalformedLocation {
        /// Instruction carrying the malformed location.
        instruction: Id,
        /// Supplied inclusive start.
        start: usize,
        /// Supplied exclusive end.
        end: usize,
    },
    /// Two instruction source ranges overlap.
    OverlappingLocations {
        /// Earlier instruction in preparation order.
        first: Id,
        /// Conflicting instruction.
        second: Id,
    },
    /// The supplied stable identity does not match the instruction policy.
    IdentityMismatch {
        /// Identity supplied with the location.
        supplied: Id,
        /// Identity derived by the instruction policy.
        derived: Id,
    },
    /// A stable instruction identity occurs more than once.
    DuplicateIdentity {
        /// Repeated identity.
        instruction: Id,
    },
    /// A branch source, region boundary, or identity target is unknown.
    UnknownInstruction {
        /// Identity that is absent from the code.
        instruction: Id,
    },
    /// A raw target lies strictly inside an instruction rather than on its boundary.
    InteriorTarget {
        /// Instruction containing the branch or region declaration.
        from: Id,
        /// Rejected raw source position.
        target: usize,
        /// Instruction whose interior contains the position.
        containing: Id,
    },
    /// A raw target is outside every instruction boundary.
    OutOfRangeTarget {
        /// Instruction containing the branch or region declaration.
        from: Id,
        /// Rejected raw source position.
        target: usize,
    },
    /// A protected region is empty or reversed.
    MalformedRegion {
        /// Declared first instruction.
        start: Id,
        /// Resolved exclusive end index.
        end_index: usize,
    },
    /// Protected regions overlap.
    OverlappingRegions {
        /// Start identity of the first region.
        first_start: Id,
        /// Start identity of the conflicting region.
        second_start: Id,
    },
}

/// Immutable, fully validated located instructions and their control metadata.
pub struct LocatedCode<P: InstructionPolicy> {
    instructions: Box<[LocatedInstruction<P::Instruction, P::InstructionId>]>,
    cursors: BTreeMap<P::InstructionId, CodeCursor>,
    targets: BTreeMap<P::InstructionId, Box<[CodeCursor]>>,
    regions: Box<[ProtectedRegion]>,
}

impl<P> LocatedCode<P>
where
    P: InstructionPolicy,
    P::InstructionId: Copy + Eq + Ord,
{
    /// Validates every location and edge before freezing the code.
    pub fn freeze(
        instructions: Vec<LocatedInstruction<P::Instruction, P::InstructionId>>,
        targets: Vec<BranchTarget<P::InstructionId>>,
        regions: Vec<RegionSpec<P::InstructionId>>,
    ) -> Result<Self, CodeError<P::InstructionId>> {
        if instructions.is_empty() {
            return Err(CodeError::Empty);
        }

        let mut cursors = BTreeMap::new();
        for (index, located) in instructions.iter().enumerate() {
            let derived = P::instruction_id(&located.instruction);
            if derived != located.id {
                return Err(CodeError::IdentityMismatch {
                    supplied: located.id,
                    derived,
                });
            }
            if cursors.insert(located.id, CodeCursor(index)).is_some() {
                return Err(CodeError::DuplicateIdentity {
                    instruction: located.id,
                });
            }
            let (unit, start, end) = located.location.range();
            if start >= end {
                return Err(CodeError::MalformedLocation {
                    instruction: located.id,
                    start,
                    end,
                });
            }
            for previous in &instructions[..index] {
                let (previous_unit, previous_start, previous_end) = previous.location.range();
                if previous.location.source() == located.location.source()
                    && previous_unit == unit
                    && start < previous_end
                    && previous_start < end
                {
                    return Err(CodeError::OverlappingLocations {
                        first: previous.id,
                        second: located.id,
                    });
                }
            }
        }

        let mut frozen_targets = BTreeMap::<P::InstructionId, Vec<CodeCursor>>::new();
        for target in targets {
            if !cursors.contains_key(&target.from) {
                return Err(CodeError::UnknownInstruction {
                    instruction: target.from,
                });
            }
            let cursor = resolve_target::<P>(&target.to, target.from, &instructions, &cursors)?;
            frozen_targets.entry(target.from).or_default().push(cursor);
        }

        let mut frozen_regions = Vec::with_capacity(regions.len());
        for region in regions {
            let start = *cursors
                .get(&region.start)
                .ok_or(CodeError::UnknownInstruction {
                    instruction: region.start,
                })?;
            let end_index = match region.end {
                Some(end) => {
                    cursors
                        .get(&end)
                        .ok_or(CodeError::UnknownInstruction { instruction: end })?
                        .0
                }
                None => instructions.len(),
            };
            if start.0 >= end_index {
                return Err(CodeError::MalformedRegion {
                    start: region.start,
                    end_index,
                });
            }
            let handler =
                resolve_target::<P>(&region.handler, region.start, &instructions, &cursors)?;
            frozen_regions.push((
                region.start,
                ProtectedRegion {
                    start,
                    end_index,
                    handler,
                },
            ));
        }
        frozen_regions.sort_by_key(|(_, region)| (region.start, usize::MAX - region.end_index));
        for pair in frozen_regions.windows(2) {
            let earlier = pair[0].1;
            let later = pair[1].1;
            let crosses = earlier.start.0 < later.start.0
                && later.start.0 < earlier.end_index
                && earlier.end_index < later.end_index;
            let same_start_not_nested =
                earlier.start == later.start && earlier.end_index == later.end_index;
            if crosses || same_start_not_nested {
                return Err(CodeError::OverlappingRegions {
                    first_start: pair[0].0,
                    second_start: pair[1].0,
                });
            }
        }

        Ok(Self {
            instructions: instructions.into_boxed_slice(),
            cursors,
            targets: frozen_targets
                .into_iter()
                .map(|(from, targets)| (from, targets.into_boxed_slice()))
                .collect(),
            regions: frozen_regions
                .into_iter()
                .map(|(_, region)| region)
                .collect(),
        })
    }

    /// Returns the entry cursor, always an instruction boundary.
    pub fn entry(&self) -> CodeCursor {
        CodeCursor(0)
    }

    /// Resolves a stable instruction identity to a valid cursor.
    pub fn cursor(&self, id: P::InstructionId) -> Option<CodeCursor> {
        self.cursors.get(&id).copied()
    }

    /// Returns the instruction addressed by `cursor`.
    pub fn instruction(
        &self,
        cursor: CodeCursor,
    ) -> &LocatedInstruction<P::Instruction, P::InstructionId> {
        &self.instructions[cursor.0]
    }

    /// Advances to the next instruction boundary, or returns `None` at code end.
    pub fn next(&self, cursor: CodeCursor) -> Option<CodeCursor> {
        (cursor.0 + 1 < self.instructions.len()).then(|| CodeCursor(cursor.0 + 1))
    }

    /// Returns every validated branch target for an instruction in declaration order.
    pub fn branch_targets(&self, from: P::InstructionId) -> &[CodeCursor] {
        self.targets.get(&from).map_or(&[], Box::as_ref)
    }

    /// Returns the immutable protected-region table.
    pub fn protected_regions(&self) -> &[ProtectedRegion] {
        &self.regions
    }

    /// Selects the most deeply nested protected region containing `cursor`.
    pub fn innermost_protected_region(&self, cursor: CodeCursor) -> Option<ProtectedRegion> {
        self.regions
            .iter()
            .copied()
            .filter(|region| region.start.0 <= cursor.0 && cursor.0 < region.end_index)
            .max_by_key(|region| region.start.0)
    }

    /// Returns the number of instructions.
    pub fn len(&self) -> usize {
        self.instructions.len()
    }

    /// Returns whether there are no instructions. Valid located code is never empty.
    pub fn is_empty(&self) -> bool {
        self.instructions.is_empty()
    }

    pub(crate) fn instructions(&self) -> &[LocatedInstruction<P::Instruction, P::InstructionId>] {
        &self.instructions
    }

    pub(crate) fn hash_structure(
        &self,
        digest: &mut Sha256,
        mut encode_instruction: impl FnMut(&P::Instruction, &mut Vec<u8>),
    ) {
        digest.update(self.instructions.len().to_le_bytes());
        for located in &self.instructions {
            let mut bytes = Vec::new();
            encode_instruction(&located.instruction, &mut bytes);
            digest.update(bytes.len().to_le_bytes());
            digest.update(bytes);
            let (unit, start, end) = located.location.range();
            digest.update([match unit {
                LocationUnit::Byte => 0,
                LocationUnit::Token => 1,
            }]);
            digest.update(start.to_le_bytes());
            digest.update(end.to_le_bytes());
            digest.update([u8::from(located.safepoint)]);
            digest.update(
                located
                    .coverage
                    .map_or(u64::MAX, |value| value.counter)
                    .to_le_bytes(),
            );
        }
        digest.update(self.targets.len().to_le_bytes());
        for (from, targets) in &self.targets {
            digest.update(self.cursors[from].0.to_le_bytes());
            digest.update(targets.len().to_le_bytes());
            for target in targets.iter() {
                digest.update(target.0.to_le_bytes());
            }
        }
        digest.update(self.regions.len().to_le_bytes());
        for region in &self.regions {
            digest.update(region.start.0.to_le_bytes());
            digest.update(region.end_index.to_le_bytes());
            digest.update(region.handler.0.to_le_bytes());
        }
    }
}

fn resolve_target<P: InstructionPolicy>(
    target: &TargetLocation<P::InstructionId>,
    from: P::InstructionId,
    instructions: &[LocatedInstruction<P::Instruction, P::InstructionId>],
    cursors: &BTreeMap<P::InstructionId, CodeCursor>,
) -> Result<CodeCursor, CodeError<P::InstructionId>>
where
    P::InstructionId: Copy + Eq + Ord,
{
    if let TargetLocation::Instruction(id) = target {
        return cursors
            .get(id)
            .copied()
            .ok_or(CodeError::UnknownInstruction { instruction: *id });
    }
    let (source, unit, position) = match target {
        TargetLocation::Byte { source, offset } => (source, LocationUnit::Byte, *offset),
        TargetLocation::Token { source, index } => (source, LocationUnit::Token, *index),
        TargetLocation::Instruction(_) => unreachable!(),
    };
    for (index, located) in instructions.iter().enumerate() {
        let (located_unit, start, end) = located.location.range();
        if located.location.source() == source && located_unit == unit {
            if position == start {
                return Ok(CodeCursor(index));
            }
            if start < position && position < end {
                return Err(CodeError::InteriorTarget {
                    from,
                    target: position,
                    containing: located.id,
                });
            }
        }
    }
    Err(CodeError::OutOfRangeTarget {
        from,
        target: position,
    })
}
