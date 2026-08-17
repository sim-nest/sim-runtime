/// The JVM storage category consumed or produced by an instruction.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JvmSlotKind {
    /// A category-one non-reference value.
    CategoryOne,
    /// A category-two value occupying two JVM slots.
    CategoryTwo,
    /// A managed JVM reference and therefore a potential GC root.
    Reference,
}
impl JvmSlotKind {
    /// Returns the JVM slot width of this value category.
    pub const fn width(self) -> usize {
        match self {
            Self::CategoryOne | Self::Reference => 1,
            Self::CategoryTwo => 2,
        }
    }
}

/// Root-set change caused by one instruction's operand-stack effects.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RootEffect {
    /// Reference operands removed from the stack.
    pub removed: usize,
    /// Reference results added to the stack.
    pub added: usize,
}

/// Primitive or reference fact retained from a verifier frame slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreparedValueGuarantee {
    /// An integer-family category-one value.
    Int,
    /// A binary32 category-one value.
    Float,
    /// A signed category-two value.
    Long,
    /// A binary64 category-two value.
    Double,
    /// The null reference, assignable to every initialized reference type.
    Null,
    /// An initialized reference with its exact verifier assignability identity.
    Reference(ReferenceType),
    /// A receiver or allocation that has not completed initialization.
    Uninitialized,
}

/// Exact verifier facts that allow one prepared instruction to omit dynamic checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedVerificationGuarantee {
    stack_width: usize,
    local_width: usize,
    stack: Box<[PreparedValueGuarantee]>,
    locals: Box<[(usize, PreparedValueGuarantee)]>,
    targets: Box<[InstructionId]>,
    handlers: Box<[PreparedCatchEntry]>,
}

impl PreparedVerificationGuarantee {
    /// Occupied operand-stack width before the instruction.
    pub const fn stack_width(&self) -> usize {
        self.stack_width
    }
    /// Fixed local-frame width proved for the instruction.
    pub const fn local_width(&self) -> usize {
        self.local_width
    }
    /// Ordered initialized and uninitialized operand categories.
    pub fn stack(&self) -> &[PreparedValueGuarantee] {
        &self.stack
    }
    /// Usable local slots and their exact categories.
    pub fn locals(&self) -> &[(usize, PreparedValueGuarantee)] {
        &self.locals
    }
    /// Resolved branch targets covered by this instruction fact.
    pub fn targets(&self) -> &[InstructionId] {
        &self.targets
    }
    /// Resolved handlers covering this instruction.
    pub fn handlers(&self) -> &[PreparedCatchEntry] {
        &self.handlers
    }
}

/// Check policy selected for a single prepared instruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreparedMicroOp {
    /// Ordinary operation retaining all runtime checks.
    Checked,
    /// Specialized operation justified by an exact current verifier fact.
    Verified(PreparedVerificationGuarantee),
}

/// Exact proof identity and converged frames offered to preparation.
pub struct VerificationPreparation<'a> {
    /// Whole-class proof produced by the verifier.
    pub proof: &'a ClassVerificationProof,
    /// Exact class expected by the method being prepared.
    pub owner: &'a ClassDefinitionId,
    /// Exact class-space revision currently observed.
    pub revision: ClassSpaceRevision,
    /// Exact verifier policy expected by the runtime.
    pub policy: ValueFingerprint,
    /// Exact structural fingerprint expected by the runtime.
    pub structural: ValueFingerprint,
    /// Stable method name plus descriptor.
    pub method: &'a str,
    /// Exact method fixpoint identity expected by the caller.
    pub method_proof: ValueFingerprint,
    /// Converged entry frames keyed by stream-local instruction identity.
    pub frames: &'a [(InstructionId, VerificationState)],
}

/// Identity of prepared method code, invalidated by either bytecode or class-space change.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedCodeIdentity {
    content: Box<[u8]>,
    revision: ClassSpaceRevision,
}

impl PreparedCodeIdentity {
    /// Creates an identity from the exact Code attribute bytes and observed class-space revision.
    pub fn new(code: &[u8], revision: ClassSpaceRevision) -> Self {
        let content = code.into();
        Self { content, revision }
    }

    /// Returns whether the prepared identity still denotes these exact inputs.
    pub fn matches(&self, code: &[u8], revision: ClassSpaceRevision) -> bool {
        self.content.as_ref() == code && self.revision == revision
    }

    /// Returns the class-space revision captured during preparation.
    pub const fn revision(&self) -> ClassSpaceRevision {
        self.revision
    }
}

/// Operand and control decisions computed once before shared-machine execution.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreparedJvmOperands {
    /// No decoded operand is needed by execution.
    None,
    /// A signed immediate constant.
    Immediate(i32),
    /// A constant-pool, field, method, class, or bootstrap site key.
    ConstantSite(u16),
    /// A local slot already widened to the machine's index type.
    Local(usize),
    /// A widened local slot and signed increment.
    Increment {
        /// Widened local slot.
        slot: usize,
        /// Signed wrapping increment.
        amount: i32,
    },
    /// One direct control target.
    Direct(InstructionId),
    /// Dense switch search structure, including its default target.
    Table {
        /// Lowest key in the dense range.
        low: i32,
        /// Target selected outside the dense range.
        default: InstructionId,
        /// Targets indexed by `key - low`.
        targets: Box<[InstructionId]>,
    },
    /// Sorted sparse switch search structure, including its default target.
    Lookup {
        /// Target selected when binary search misses.
        default: InstructionId,
        /// Sorted key-to-target pairs.
        pairs: Box<[(i32, InstructionId)]>,
    },
    /// Immutable admissible whole-value shuffle layouts.
    Shuffle(&'static [(&'static [usize], &'static [usize])]),
    /// Decoded operands retained for instruction families not yet executed by this module.
    Other(Box<[InstructionOperand]>),
}

/// One classfile exception-table row resolved entirely to prepared instruction identities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PreparedCatchEntry {
    /// Original exception-table row, preserving classfile search order.
    pub row: usize,
    /// First protected instruction, inclusive.
    pub start: InstructionId,
    /// First instruction after the protected range, or `None` at code end.
    pub end: Option<InstructionId>,
    /// Prepared handler entry instruction.
    pub handler: InstructionId,
    /// Constant-pool catch class, or zero for a catch-all handler.
    pub catch_type: u16,
}

/// Complete execution metadata required before a JVM instruction can run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JvmInstructionSemantics {
    /// Operand categories popped in JVM evaluation order.
    pub pops: &'static [JvmSlotKind],
    /// Result categories pushed in JVM evaluation order.
    pub pushes: &'static [JvmSlotKind],
    /// Whether execution must poll managed-runtime state before this instruction.
    pub safepoint: bool,
}

impl JvmInstructionSemantics {
    /// Returns the total input width in JVM slots.
    pub fn input_width(self) -> usize {
        self.pops.iter().copied().map(JvmSlotKind::width).sum()
    }

    /// Returns the total output width in JVM slots.
    pub fn output_width(self) -> usize {
        self.pushes.iter().copied().map(JvmSlotKind::width).sum()
    }

    /// Derives the managed-root change from the declared value categories.
    pub fn root_effect(self) -> RootEffect {
        RootEffect {
            removed: self
                .pops
                .iter()
                .filter(|kind| **kind == JvmSlotKind::Reference)
                .count(),
            added: self
                .pushes
                .iter()
                .filter(|kind| **kind == JvmSlotKind::Reference)
                .count(),
        }
    }
}

/// Supplies executable semantics for manifest-admitted classfile opcodes.
///
/// This is deliberately a coverage policy rather than a second opcode table. Implementations
/// match the shared [`Opcode`] identity and preparation refuses every identity they omit.
pub trait JvmInstructionPolicy {
    /// Returns executable semantics when this runtime implements `opcode`.
    fn semantics(opcode: Opcode) -> Option<JvmInstructionSemantics>;
}

/// A decoded JVM instruction with all execution metadata frozen beside it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedJvmInstruction {
    id: InstructionId,
    opcode: Opcode,
    dispatch: PreparedDispatchFamily,
    instruction: Instruction,
    input_width: usize,
    output_width: usize,
    root_effect: RootEffect,
    operands: PreparedJvmOperands,
    work_charge: usize,
    code_identity: Option<PreparedCodeIdentity>,
    handler_membership: Box<[PreparedCatchEntry]>,
    handler_entries: Box<[usize]>,
    micro_op: PreparedMicroOp,
}

impl PreparedJvmInstruction {
    /// Returns the stream-local instruction identity.
    pub const fn id(&self) -> InstructionId {
        self.id
    }

    /// Returns the shared, generated classfile opcode identity.
    pub const fn opcode(&self) -> Opcode {
        self.opcode
    }

    /// Returns the dense execution family frozen during preparation.
    pub const fn dispatch_family(&self) -> PreparedDispatchFamily {
        self.dispatch
    }

    /// Returns the losslessly decoded instruction and operands.
    pub fn instruction(&self) -> &Instruction {
        &self.instruction
    }

    /// Returns the operand width consumed in JVM slots.
    pub const fn input_width(&self) -> usize {
        self.input_width
    }

    /// Returns the result width produced in JVM slots.
    pub const fn output_width(&self) -> usize {
        self.output_width
    }

    /// Returns the instruction's managed-root stack effect.
    pub const fn root_effect(&self) -> RootEffect {
        self.root_effect
    }

    /// Returns the fully lowered operands and control search structure.
    pub fn prepared_operands(&self) -> &PreparedJvmOperands {
        &self.operands
    }

    /// Returns the immutable work charged after successful execution.
    pub const fn work_charge(&self) -> usize {
        self.work_charge
    }

    /// Returns the exact classfile-content and class-space identity, when supplied by the loader.
    pub fn code_identity(&self) -> Option<&PreparedCodeIdentity> {
        self.code_identity.as_ref()
    }

    /// Returns exception-table rows whose protected range contains this instruction.
    pub fn handler_membership(&self) -> &[PreparedCatchEntry] {
        &self.handler_membership
    }

    /// Returns exception-table rows that enter at this instruction.
    pub fn handler_entries(&self) -> &[usize] {
        &self.handler_entries
    }

    /// Returns the checked or exactly verified operation selected during preparation.
    pub fn micro_op(&self) -> &PreparedMicroOp {
        &self.micro_op
    }
}
