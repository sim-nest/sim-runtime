//! Bounded, immutable inspection projections for JVM verification evidence.

use crate::{
    ClassVerificationProof, MethodVerificationError, MethodVerificationProof,
    StackMapConstraintError, VerificationFrame,
};

/// A bounded read-only projection of one verifier frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationFrameView {
    /// Whether a predecessor reached the frame.
    pub reachable: bool,
    /// Total physical slot capacity, including category-2 tails.
    pub capacity: usize,
    /// Debug-stable values beginning in the visible prefix of slots.
    pub slots: Box<[Option<String>]>,
    /// Number of physical slots omitted by the caller's bound.
    pub omitted: usize,
}

impl VerificationFrameView {
    /// Reads at most `limit` physical slots without exposing mutable verifier state.
    pub fn bounded(frame: &VerificationFrame, limit: usize) -> Self {
        let visible = frame.capacity().min(limit);
        Self {
            reachable: matches!(frame, VerificationFrame::Reachable { .. }),
            capacity: frame.capacity(),
            slots: (0..visible)
                .map(|index| frame.get(index).map(|value| format!("{value:?}")))
                .collect(),
            omitted: frame.capacity() - visible,
        }
    }
}

/// Bounded public facts from a sealed whole-method proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MethodVerificationView {
    /// Stable completion-proof identity.
    pub proof: String,
    /// Dependency identities, bounded by the caller.
    pub dependencies: Box<[String]>,
    /// Unreachable exception-table rows, bounded by the caller.
    pub unreachable_handlers: Box<[usize]>,
    /// Total rows omitted across the two bounded collections.
    pub omitted: usize,
}

impl MethodVerificationView {
    /// Projects a sealed proof. There is deliberately no constructor from a boolean flag.
    pub fn bounded(proof: &MethodVerificationProof, limit: usize) -> Self {
        let dependencies = proof.dependencies();
        let handlers = proof.unreachable_handlers();
        Self {
            proof: format!("{:?}", proof.fixpoint()),
            dependencies: dependencies
                .iter()
                .take(limit)
                .map(|value| format!("{value:?}"))
                .collect(),
            unreachable_handlers: handlers.iter().copied().take(limit).collect(),
            omitted: dependencies.len().saturating_sub(limit)
                + handlers.len().saturating_sub(limit),
        }
    }
}

/// Bounded public facts from an exact sealed class proof.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassVerificationView {
    /// Exact class definition identity.
    pub owner: String,
    /// Stable content identity of the complete class proof.
    pub proof: String,
    /// Declared method identities paired with their proof identities.
    pub methods: Box<[(String, String)]>,
    /// Number of method rows omitted by the caller's bound.
    pub omitted: usize,
}

impl ClassVerificationView {
    /// Projects a sealed proof without exposing a proof constructor or mutable cache entry.
    pub fn bounded(proof: &ClassVerificationProof, limit: usize) -> Self {
        Self {
            owner: format!("{:?}", proof.owner()),
            proof: format!("{:?}", proof.identity()),
            methods: proof
                .methods()
                .iter()
                .take(limit)
                .map(|method| (method.method().to_owned(), format!("{:?}", method.proof())))
                .collect(),
            omitted: proof.methods().len().saturating_sub(limit),
        }
    }
}

/// Stable debugger-free explanation of a verification refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationExplanation {
    /// Stable machine-readable refusal category.
    pub code: &'static str,
    /// Human-readable reason retaining the relevant row or instruction identity.
    pub reason: String,
}

impl VerificationExplanation {
    /// Converts a typed refusal into a bounded, non-recursive explanation.
    pub fn for_method(error: &MethodVerificationError) -> Self {
        let (code, reason) = match error {
            MethodVerificationError::IncompleteFixpoint(value) => (
                "incomplete-fixpoint",
                format!("completion proof mismatch: {value:?}"),
            ),
            MethodVerificationError::UnresolvedCatchType { row, catch_type } => (
                "unresolved-catch",
                format!("exception row {row} names unresolved constant #{catch_type}"),
            ),
            MethodVerificationError::CatchTypeNotThrowable { row, catch_type } => (
                "catch-not-throwable",
                format!("exception row {row} constant #{catch_type} is not Throwable"),
            ),
            MethodVerificationError::CatchTypeQuery { row, error } => (
                "catch-query",
                format!("exception row {row} hierarchy query failed: {error:?}"),
            ),
            MethodVerificationError::ExceptionalFrame { row, instruction } => (
                "exceptional-frame",
                format!("exception row {row} has no precise frame at {instruction:?}"),
            ),
            MethodVerificationError::TargetConstraint(constraint) => match constraint {
                StackMapConstraintError::Missing { instruction } => (
                    "missing-stack-map",
                    format!("target {instruction:?} requires a declared frame"),
                ),
                StackMapConstraintError::NotAssignable { instruction } => (
                    "stack-map-not-assignable",
                    format!(
                        "inferred frame at {instruction:?} is not assignable to its declaration"
                    ),
                ),
                StackMapConstraintError::MissingInference { instruction } => (
                    "missing-inference",
                    format!("target {instruction:?} has no converged inferred frame"),
                ),
            },
            MethodVerificationError::UnreachableHandler { row } => (
                "unreachable-handler",
                format!("exception row {row} is unreachable under the selected policy"),
            ),
        };
        Self { code, reason }
    }
}

/// Source-generated verifier-rule coverage facts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierCoverage {
    /// Byte-indexed opcode rows owned by exactly one rule family.
    pub opcode_rows: usize,
    /// Distinct verifier rule families represented in the generated table.
    pub rule_families: usize,
    /// Manifest that owns the generated inventory.
    pub source: &'static str,
}
