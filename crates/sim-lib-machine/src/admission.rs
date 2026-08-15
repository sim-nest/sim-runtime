use sha2::{Digest, Sha256};
use sim_kernel::{ContentId, Symbol};

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

impl AdmissionLimits {
    fn encode(self, digest: &mut Sha256) {
        for value in [
            self.instructions,
            self.operand_units,
            self.slots,
            self.frames,
            self.work,
        ] {
            digest.update(value.to_le_bytes());
        }
    }
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

/// Pure consumer checks and canonical encoding used during admission.
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

    /// Appends a canonical, unambiguous encoding of consumer-owned metadata.
    fn encode_metadata(metadata: &M, output: &mut Vec<u8>);

    /// Appends a canonical, unambiguous encoding of one decoded instruction.
    fn encode_instruction(instruction: &P::Instruction, output: &mut Vec<u8>);
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
        Ok(Self {
            content_id: content_id::<P, M, A>(description),
        })
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
        self.content_id == content_id::<P, M, A>(description)
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

fn content_id<P, M, A>(description: &MachineDescription<'_, P, M>) -> ContentId
where
    P: InstructionPolicy,
    P::InstructionId: Copy + Eq + Ord,
    A: AdmissionPolicy<P, M>,
{
    let mut digest = Sha256::new();
    digest.update(b"sim-lib-machine/admission/v1\0");
    description.limits.encode(&mut digest);
    let mut encoded = Vec::new();
    A::encode_metadata(description.metadata, &mut encoded);
    hash_field(&mut digest, &encoded);
    description
        .code
        .hash_structure(&mut digest, |instruction, output| {
            A::encode_instruction(instruction, output);
        });
    ContentId::from_bytes(
        Symbol::qualified("core", "sha256"),
        digest.finalize().into(),
    )
}

fn hash_field(digest: &mut Sha256, bytes: &[u8]) {
    digest.update(bytes.len().to_le_bytes());
    digest.update(bytes);
}
