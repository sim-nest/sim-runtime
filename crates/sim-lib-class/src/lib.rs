//! Concrete language-neutral class semantics for SIM.
//!
//! This crate deliberately begins with an executable semantic boundary. The
//! inventory prevents neighboring object models from being mistaken for class
//! inheritance, while characterization scenarios freeze the behavior that the
//! checked descriptor implementation must preserve.

mod cache;
mod characterization;
mod descriptor;
mod inventory;
mod lineage;

pub use cache::{
    CacheAccess, CacheAccessKind, CacheError, CacheRevisions, ClassCache, ClassRoot,
    DerivedClassView, SnapshotGraph,
};
pub use characterization::{
    CharacterizationScenario, ExpectedOutcome, FailureMode, ScenarioKind,
    characterization_scenarios, scenario_content_id,
};
pub use descriptor::{
    ClassDescriptor, ClassDescriptorError, ClassDescriptorInput, ClassIdentity, DeclaredParent,
    MemberShape, OpenMetadataEntry, ReadConstruction,
};
pub use inventory::{
    CandidateDisposition, CandidateModel, ExclusionReason, ParentMeaning, SemanticDomain,
    candidate_inventory, exclusion_ledger, non_goals,
};
pub use lineage::{
    C3Policy, DeclaredOrderPolicy, LineageBudget, LineageError, LineageGraph, LineagePolicy,
    PrecedenceConstraint,
};

#[cfg(test)]
mod tests;
