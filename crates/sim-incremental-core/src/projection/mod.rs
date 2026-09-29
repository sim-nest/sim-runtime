//! Durable semantic projections over sealed, caller-supplied world facts.
//!
//! Projection is deliberately separate from observation and execution. A
//! provider receives an immutable selected view, has no I/O port, and returns a
//! canonical value plus the exact facts it consumed. Admission proves which
//! implementation and input policy may produce reusable results; confinement
//! records effect bounds independently.

mod admission;
mod assay;
mod assay_repair;
mod builtin;
mod engine;
mod graph;
mod model;

// NativeSourceEvidence, ProjectorQualificationVerifier, and
// bootstrap_native_source are `pub(crate)`, deliberately not re-exported: no
// downstream crate can construct evidence, call the verifier, or mint a
// bootstrap receipt. The only route to a ProjectorQualification is
// `ProjectionRegistry::qualification_for`. See admission.rs's module docs.
pub use admission::{ClosedWasmEvidence, QualificationError};
#[cfg(test)]
use admission::{ProjectorQualificationVerifier, bootstrap_native_source};
pub use assay::{
    AssayContract, AssayError, AssayOutcome, ControlledDelta, ControlledDeltaClass,
    ExpectedClosure, ExpectedClosureSet, ExpectedClosureViolation, PredictedClosureAssay,
    PredictedClosureReport, PredictedWork, StageOneQualification,
};
pub use assay_repair::{
    ArchitectureFaultReview, ProjectionRepairItem, ProjectionRepairSet, ProjectionRepairTracker,
    RepairDisposition,
};
pub use builtin::{
    BASELINE_PROJECTION_KINDS, PathSelectionRules, SelectFactsProvider, install_baseline_providers,
};
pub use engine::{ConfigShapeVerifier, ProjectionEngine, ProjectionRegistry};
pub use graph::{ClosureError, FederatedClosure, OwnerProjectionGraph};
pub use model::{
    ConclusionId, ConfinementEvidence, DeclaredInputSelector, DeterministicImportManifest,
    ExecutionSemantics, Explanation, FactId, MediatedAccessWitness, ObservedFact, ObservedWorld,
    PackageIdentity, ProjectionBudget, ProjectionDigest, ProjectionError, ProjectionInputs,
    ProjectionKindRef, ProjectionOutput, ProjectionProvider, ProjectionResult, ProjectionSpec,
    ProjectorPolicy, ProjectorQualification, QualifiedRuntime, QualifiedSourceClosure,
};
// ProjectorQualificationKind is `pub(crate)`: only this crate's own admission
// logic constructs a ProjectorQualification's contents. Not re-exported.
use model::ProjectorQualificationKind;

#[cfg(test)]
mod assay_tests;
#[cfg(test)]
mod tests;
