use sim_kernel::{ContentId, Datum, NumberLiteral, Symbol};

use crate::{InstructionPolicy, LocatedCode};

/// Hard bounds admitted for one machine description.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmissionLimits {
    /// Greatest number of decoded instructions.
    pub instructions: usize,
    /// Greatest logical operand width.
    pub operand_units: usize,
    /// Greatest number of indexed slots.
    pub slots: usize,
    /// Greatest guest frame depth.
    pub frames: usize,
    /// Greatest work allowance for one drive operation.
    pub work: usize,
}

/// Immutable code plus the consumer-owned metadata needed to admit it.
pub struct MachineDescription<'a, P: InstructionPolicy, M> {
    code: &'a LocatedCode<P>,
    limits: AdmissionLimits,
    metadata: &'a M,
}

impl<'a, P: InstructionPolicy, M> MachineDescription<'a, P, M> {
    /// Describes already-frozen code under explicit machine limits.
    pub fn new(code: &'a LocatedCode<P>, limits: AdmissionLimits, metadata: &'a M) -> Self {
        Self {
            code,
            limits,
            metadata,
        }
    }

    /// Returns the immutable located code.
    pub fn code(&self) -> &LocatedCode<P> {
        self.code
    }

    /// Returns the declared limits.
    pub fn limits(&self) -> AdmissionLimits {
        self.limits
    }

    /// Returns consumer-owned entry and policy metadata.
    pub fn metadata(&self) -> &M {
        self.metadata
    }
}

/// Pure consumer checks and canonical semantic data used during admission.
///
/// These callbacks validate data only. Effect classification and execution are
/// intentionally not part of this trait, so admission cannot invoke them.
/// A WebAssembly validator or an eBPF verifier can supply the policy.
pub trait AdmissionPolicy<P: InstructionPolicy, M> {
    /// Structured consumer refusal.
    type Refusal;

    /// Checks the machine-wide description, including entry shape and policy compatibility.
    fn validate_description(
        description: &MachineDescription<'_, P, M>,
    ) -> Result<(), Self::Refusal>;

    /// Checks one instruction. Calling this for every instruction proves coverage.
    fn validate_instruction(
        instruction: &P::Instruction,
        metadata: &M,
    ) -> Result<(), Self::Refusal>;

    /// Stable identity of this admission policy.
    fn policy_identity() -> Symbol;

    /// Version of the policy semantics and its datum projections.
    fn policy_version() -> u32;

    /// Projects consumer-owned metadata to canonical semantic data.
    fn metadata_datum(metadata: &M) -> Datum;

    /// Projects one decoded instruction to canonical semantic data.
    fn instruction_datum(instruction: &P::Instruction) -> Datum;
}

/// A refusal produced before a permit can exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdmissionError<R> {
    /// A declared limit is zero.
    ZeroLimit {
        /// Name of the invalid limit.
        limit: &'static str,
    },
    /// Located code exceeds the declared instruction bound.
    InstructionLimit {
        /// Number of located instructions.
        actual: usize,
        /// Declared maximum.
        limit: usize,
    },
    /// A pure consumer validation rejected the description.
    Policy(R),
    /// The policy supplied data that cannot be canonically identified.
    NonCanonical,
}

/// Proof that one exact immutable machine description passed admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachinePermit {
    content_id: ContentId,
}

impl MachinePermit {
    /// Validates the complete description and mints its content-bound permit.
    pub fn admit<P, M, A>(
        description: &MachineDescription<'_, P, M>,
    ) -> Result<Self, AdmissionError<A::Refusal>>
    where
        P: InstructionPolicy,
        P::InstructionId: Copy + Eq + Ord,
        A: AdmissionPolicy<P, M>,
    {
        validate_limits(description)?;
        A::validate_description(description).map_err(AdmissionError::Policy)?;
        for located in description.code.instructions() {
            A::validate_instruction(located.instruction(), description.metadata)
                .map_err(AdmissionError::Policy)?;
        }
        let content_id =
            content_id::<P, M, A>(description).map_err(|_| AdmissionError::NonCanonical)?;
        Ok(Self { content_id })
    }

    /// Returns the kernel content identity admitted by this permit.
    pub fn content_id(&self) -> &ContentId {
        &self.content_id
    }

    /// Returns whether this permit binds exactly the supplied description.
    pub fn accepts<P, M, A>(&self, description: &MachineDescription<'_, P, M>) -> bool
    where
        P: InstructionPolicy,
        P::InstructionId: Copy + Eq + Ord,
        A: AdmissionPolicy<P, M>,
    {
        content_id::<P, M, A>(description).is_ok_and(|id| self.content_id == id)
    }
}

fn validate_limits<P: InstructionPolicy, M, R>(
    description: &MachineDescription<'_, P, M>,
) -> Result<(), AdmissionError<R>> {
    for (limit, value) in [
        ("instructions", description.limits.instructions),
        ("operand_units", description.limits.operand_units),
        ("slots", description.limits.slots),
        ("frames", description.limits.frames),
        ("work", description.limits.work),
    ] {
        if value == 0 {
            return Err(AdmissionError::ZeroLimit { limit });
        }
    }
    if description.code.len() > description.limits.instructions {
        return Err(AdmissionError::InstructionLimit {
            actual: description.code.len(),
            limit: description.limits.instructions,
        });
    }
    Ok(())
}

fn content_id<P, M, A>(description: &MachineDescription<'_, P, M>) -> sim_kernel::Result<ContentId>
where
    P: InstructionPolicy,
    P::InstructionId: Copy + Eq + Ord,
    A: AdmissionPolicy<P, M>,
{
    Datum::Node {
        tag: Symbol::qualified("machine", "AdmissionIdentityV2"),
        fields: vec![
            (Symbol::new("policy"), Datum::Symbol(A::policy_identity())),
            (
                Symbol::new("policy-version"),
                usize_datum(A::policy_version() as usize),
            ),
            (Symbol::new("limits"), limits_datum(description.limits)),
            (
                Symbol::new("metadata"),
                A::metadata_datum(description.metadata),
            ),
            (
                Symbol::new("located-code"),
                description.code.semantic_datum(A::instruction_datum),
            ),
        ],
    }
    .content_id()
}

fn limits_datum(limits: AdmissionLimits) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("machine", "AdmissionLimitsV1"),
        fields: vec![
            (
                Symbol::new("instructions"),
                usize_datum(limits.instructions),
            ),
            (
                Symbol::new("operand-units"),
                usize_datum(limits.operand_units),
            ),
            (Symbol::new("slots"), usize_datum(limits.slots)),
            (Symbol::new("frames"), usize_datum(limits.frames)),
            (Symbol::new("work"), usize_datum(limits.work)),
        ],
    }
}

pub(crate) fn usize_datum(value: usize) -> Datum {
    Datum::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "usize"),
        canonical: value.to_string(),
    })
}
