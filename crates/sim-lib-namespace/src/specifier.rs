//! Bounded, authority-free policy for source-module specifiers.

use std::fmt;

use super::ModuleIdentity;

/// Maximum number of textual candidates admitted to one policy decision.
pub const MAX_SPECIFIER_CANDIDATES: usize = 16;

/// Maximum UTF-8 byte length of one candidate specifier.
pub const MAX_SPECIFIER_BYTES: usize = 4_096;

/// Data-only input to a [`ModuleSpecifierPolicy`].
///
/// Deliberately absent are a source root, runtime context, and authority. A
/// policy may select or normalize text, but cannot probe storage or acquire a
/// capability through this interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecifierPolicyRequest {
    importer: Option<ModuleIdentity>,
    candidates: Vec<String>,
}

impl SpecifierPolicyRequest {
    /// Constructs a bounded policy request, or an exact refusal when its shape
    /// exceeds the seam's fixed limits.
    pub fn new(
        importer: Option<ModuleIdentity>,
        candidates: Vec<String>,
    ) -> std::result::Result<Self, SpecifierRefusal> {
        if candidates.is_empty() {
            return Err(SpecifierRefusal::new(
                SpecifierRefusalCode::NoCandidates,
                "module specifier policy requires at least one candidate",
            ));
        }
        if candidates.len() > MAX_SPECIFIER_CANDIDATES {
            return Err(SpecifierRefusal::new(
                SpecifierRefusalCode::TooManyCandidates,
                format!(
                    "module specifier policy candidate count {} exceeds {}",
                    candidates.len(),
                    MAX_SPECIFIER_CANDIDATES
                ),
            ));
        }
        if let Some(candidate) = candidates
            .iter()
            .find(|candidate| candidate.len() > MAX_SPECIFIER_BYTES)
        {
            return Err(SpecifierRefusal::new(
                SpecifierRefusalCode::CandidateTooLong,
                format!(
                    "module specifier policy candidate length {} exceeds {} bytes",
                    candidate.len(),
                    MAX_SPECIFIER_BYTES
                ),
            ));
        }
        Ok(Self {
            importer,
            candidates,
        })
    }

    /// Importer identity available for language-specific textual normalization.
    pub fn importer(&self) -> Option<&ModuleIdentity> {
        self.importer.as_ref()
    }

    /// Bounded candidate texts, in caller-declared order.
    pub fn candidates(&self) -> &[String] {
        &self.candidates
    }
}

/// One normalized request selected by a specifier policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedSpecifier(String);

impl NormalizedSpecifier {
    /// Creates a normalized textual request.
    pub fn new(text: String) -> std::result::Result<Self, SpecifierRefusal> {
        if text.len() > MAX_SPECIFIER_BYTES {
            return Err(SpecifierRefusal::new(
                SpecifierRefusalCode::CandidateTooLong,
                format!(
                    "normalized module specifier length {} exceeds {} bytes",
                    text.len(),
                    MAX_SPECIFIER_BYTES
                ),
            ));
        }
        Ok(Self(text))
    }

    /// Normalized specifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable category for an exact policy refusal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecifierRefusalCode {
    /// No candidate was supplied.
    NoCandidates,
    /// More candidates were supplied than the seam admits.
    TooManyCandidates,
    /// A candidate or normalized result exceeded the text bound.
    CandidateTooLong,
    /// The installed identity policy was asked to choose among candidates.
    IdentityRequiresOneCandidate,
    /// A custom policy refused the candidate set.
    PolicyRefused,
}

/// Exact, inspectable refusal returned instead of a normalized request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpecifierRefusal {
    code: SpecifierRefusalCode,
    detail: String,
}

impl SpecifierRefusal {
    /// Creates an exact refusal with a stable category and detail.
    pub fn new(code: SpecifierRefusalCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }

    /// Stable refusal category.
    pub fn code(&self) -> SpecifierRefusalCode {
        self.code
    }

    /// Stable refusal detail.
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl fmt::Display for SpecifierRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

/// Chooses exactly one bounded normalized request, or refuses exactly.
pub trait ModuleSpecifierPolicy: Send + Sync {
    /// Resolve bounded text without access to roots, capabilities, or a runtime context.
    fn resolve(
        &self,
        request: &SpecifierPolicyRequest,
    ) -> std::result::Result<NormalizedSpecifier, SpecifierRefusal>;
}

/// Current behavior: preserve the sole supplied specifier byte-for-byte.
#[derive(Clone, Copy, Debug, Default)]
pub struct IdentitySpecifierPolicy;

impl ModuleSpecifierPolicy for IdentitySpecifierPolicy {
    fn resolve(
        &self,
        request: &SpecifierPolicyRequest,
    ) -> std::result::Result<NormalizedSpecifier, SpecifierRefusal> {
        if request.candidates.len() != 1 {
            return Err(SpecifierRefusal::new(
                SpecifierRefusalCode::IdentityRequiresOneCandidate,
                format!(
                    "identity module specifier policy requires exactly one candidate, got {}",
                    request.candidates.len()
                ),
            ));
        }
        NormalizedSpecifier::new(request.candidates[0].clone())
    }
}
