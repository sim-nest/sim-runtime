//! Durable semantic projections over sealed, caller-supplied world facts.
//!
//! Projection is deliberately separate from observation and execution. A
//! provider receives an immutable selected view, has no I/O port, and returns a
//! canonical value plus the exact facts it consumed. Admission proves which
//! implementation and input policy may produce reusable results; confinement
//! records effect bounds independently.

mod admission;
mod builtin;
mod engine;
mod graph;
mod model;

pub use admission::{
    ClosedWasmEvidence, NativeSourceEvidence, ProjectorQualificationVerifier, QualificationError,
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

#[cfg(test)]
mod tests;
