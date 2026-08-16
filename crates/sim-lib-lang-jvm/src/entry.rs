//! One typed, revision-bound admission path for every JVM execution target.

use std::{error::Error, fmt, sync::Arc};

use sim_kernel::ContentId;
use sim_lib_machine::MachinePermit;

use crate::{ClassDefinition, ClassDefinitionId, ClassLoader, ClassSpaceRevision};

/// The three target families which must share method-entry admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryTarget {
    /// A classfile-declared method.
    Method {
        /// Exact JVMS member name.
        name: String,
        /// Exact JVMS method descriptor.
        descriptor: String,
    },
    /// A closed-table runtime intrinsic.
    Intrinsic {
        /// Exact declared member name selected by the intrinsic table.
        name: String,
        /// Exact declared method descriptor.
        descriptor: String,
    },
    /// A dynamically selected target, already resolved to an exact member.
    Dynamic {
        /// Exact member name produced by dynamic resolution.
        name: String,
        /// Exact method descriptor produced by dynamic resolution.
        descriptor: String,
    },
}

impl EntryTarget {
    fn member(&self) -> (&str, &str) {
        match self {
            Self::Method { name, descriptor }
            | Self::Intrinsic { name, descriptor }
            | Self::Dynamic { name, descriptor } => (name, descriptor),
        }
    }
}

/// Fidelity established before preparation. This phase deliberately promises no verifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VerificationFidelity {
    /// Structural classfile checks, resolution, and static admission completed.
    StaticChecked,
    /// A provider additionally supplied a verifier proof.
    Verified,
}

/// Optional verifier extension point. Implementations inspect immutable inputs only.
pub trait VerifierProvider {
    /// Provider-owned immutable proof.
    type Proof;
    /// Produces a proof, or declines while retaining `static-checked` fidelity.
    fn verify(
        &self,
        class: &ClassDefinition,
        target: &EntryTarget,
        machine_content: &ContentId,
    ) -> Result<Option<Self::Proof>, EntryRefusal>;
}

/// The intentionally absent verifier implementation for the current profile.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoVerifier;

impl VerifierProvider for NoVerifier {
    type Proof = ();

    fn verify(
        &self,
        _class: &ClassDefinition,
        _target: &EntryTarget,
        _machine_content: &ContentId,
    ) -> Result<Option<Self::Proof>, EntryRefusal> {
        Ok(None)
    }
}

/// Located refusal from the pure admission path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryRefusal {
    /// The definition belongs to another loader namespace.
    ForeignClass {
        /// Definition rejected at the loader boundary.
        class: ClassDefinitionId,
    },
    /// Static resolution found no exact declared method.
    MissingMethod {
        /// Definition searched during static resolution.
        class: ClassDefinitionId,
        /// Requested member name.
        name: String,
        /// Requested method descriptor.
        descriptor: String,
    },
    /// A permit's class-space identity no longer equals the live identity.
    StaleClassSpace {
        /// Definition whose prepared entry became stale.
        class: ClassDefinitionId,
        /// Exact class-space identity carried by the permit.
        admitted: ClassSpaceRevision,
        /// Live class-space identity found before execution.
        current: ClassSpaceRevision,
    },
}

impl fmt::Display for EntryRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignClass { class } => {
                write!(f, "class {} belongs to another loader", class.binary_name())
            }
            Self::MissingMethod {
                class,
                name,
                descriptor,
            } => write!(
                f,
                "{}.{}{} is not declared",
                class.binary_name(),
                name,
                descriptor
            ),
            Self::StaleClassSpace {
                class,
                admitted,
                current,
            } => write!(
                f,
                "{} was admitted at class-space revision {} but current revision is {}",
                class.binary_name(),
                admitted.number(),
                current.number()
            ),
        }
    }
}

impl Error for EntryRefusal {}

/// Classfile validation evidence bound to content and class-space revision.
pub struct ClassfilePermit<'a> {
    loader: &'a ClassLoader,
    class: Arc<ClassDefinition>,
    revision: ClassSpaceRevision,
}
/// Resolved target bound to the preceding classfile permit.
pub struct ResolvedEntry<'a> {
    permit: ClassfilePermit<'a>,
    target: EntryTarget,
}
/// Statically admitted target bound to machine content.
pub struct StaticAdmission<'a> {
    resolved: ResolvedEntry<'a>,
    machine_content: ContentId,
}
/// Optional verifier result, retaining the explicit fidelity tier.
pub struct Verification<'a, P> {
    admission: StaticAdmission<'a>,
    proof: Option<P>,
    fidelity: VerificationFidelity,
}
/// Pure preparation token; it allocates no guest object and writes no guest static.
pub struct PreparedEntry<'a, P> {
    verification: Verification<'a, P>,
}
/// The only token from which JVM effects may be driven.
pub struct ExecutionPermit<'a, P> {
    prepared: PreparedEntry<'a, P>,
}

impl<'a> ClassfilePermit<'a> {
    /// Binds an already decoded and validated definition to the live class-space identity.
    pub fn new(loader: &'a ClassLoader, class: Arc<ClassDefinition>) -> Result<Self, EntryRefusal> {
        if class.id().loader() != loader.id() {
            return Err(EntryRefusal::ForeignClass {
                class: class.id().clone(),
            });
        }
        Ok(Self {
            loader,
            class,
            revision: loader.revision(),
        })
    }

    /// Resolves every target family through the same exact member lookup.
    pub fn resolve(self, target: EntryTarget) -> Result<ResolvedEntry<'a>, EntryRefusal> {
        let (name, descriptor) = target.member();
        if self
            .class
            .metadata()
            .select_method(name, descriptor)
            .is_none()
        {
            return Err(EntryRefusal::MissingMethod {
                class: self.class.id().clone(),
                name: name.into(),
                descriptor: descriptor.into(),
            });
        }
        Ok(ResolvedEntry {
            permit: self,
            target,
        })
    }
}

impl<'a> ResolvedEntry<'a> {
    /// Composes the machine's pure static admission proof.
    pub fn admit(self, machine: &MachinePermit) -> StaticAdmission<'a> {
        StaticAdmission {
            resolved: self,
            machine_content: machine.content_id().clone(),
        }
    }
}

impl<'a> StaticAdmission<'a> {
    /// Invokes the optional pure verifier seam and advances to preparation.
    pub fn verify<V: VerifierProvider>(
        self,
        provider: &V,
    ) -> Result<PreparedEntry<'a, V::Proof>, EntryRefusal> {
        let proof = provider.verify(
            &self.resolved.permit.class,
            &self.resolved.target,
            &self.machine_content,
        )?;
        let fidelity = if proof.is_some() {
            VerificationFidelity::Verified
        } else {
            VerificationFidelity::StaticChecked
        };
        Ok(PreparedEntry {
            verification: Verification {
                admission: self,
                proof,
                fidelity,
            },
        })
    }
}

impl<'a, P> PreparedEntry<'a, P> {
    /// Mints the final execution permit without performing a guest effect.
    pub fn permit(self) -> ExecutionPermit<'a, P> {
        ExecutionPermit { prepared: self }
    }
}

impl<P> ExecutionPermit<'_, P> {
    /// Fidelity carried by this exact permit.
    pub const fn fidelity(&self) -> VerificationFidelity {
        self.prepared.verification.fidelity
    }

    /// Optional provider proof retained beside the final permit.
    pub fn verifier_proof(&self) -> Option<&P> {
        self.prepared.verification.proof.as_ref()
    }

    /// Exact machine content identity admitted for this entry.
    pub fn machine_content(&self) -> &ContentId {
        &self.prepared.verification.admission.machine_content
    }

    /// Target family and exact member selected by the shared pipeline.
    pub fn target(&self) -> &EntryTarget {
        &self.prepared.verification.admission.resolved.target
    }
}

/// The sole JVM drive entry point. Revision identity is checked before `effect` can run.
pub fn drive<P, T>(
    permit: ExecutionPermit<'_, P>,
    effect: impl FnOnce() -> T,
) -> Result<T, EntryRefusal> {
    let admission = &permit.prepared.verification.admission;
    let current = admission.resolved.permit.loader.revision();
    let admitted = admission.resolved.permit.revision;
    if current != admitted {
        return Err(EntryRefusal::StaleClassSpace {
            class: admission.resolved.permit.class.id().clone(),
            admitted,
            current,
        });
    }
    Ok(effect())
}
