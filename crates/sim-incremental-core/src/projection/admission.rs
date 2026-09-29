use std::{error::Error, fmt};

use sim_conformance_core::{
    CheckArgument, CheckInputClosureId, CheckScopeId, CheckTemplate, CheckedSubjectId,
    CheckerBinding, CheckerResultId, CommandId, ConformancePackId, EnvironmentPolicyId,
    EvidenceGrade, EvidenceProvenanceId, EvidenceSetId, LiveCheckerAuthority, LiveCheckerOwner,
    LiveCheckerReceipt, OutputShapeId, OwnerBindingId, PolicyId, ProofCodeId, RevocationSourceId,
    WorkingDirectoryPolicyId,
};
use sim_kernel::{ContentId, Datum, NumberLiteral, Symbol};

use super::{
    DeterministicImportManifest, ProjectorPolicy, ProjectorQualification, QualifiedRuntime,
    QualifiedSourceClosure,
};

/// Live, receipt-backed evidence that an exact native implementation was
/// checked before this qualification attempt.
///
/// There is no field here a caller can simply set to claim review happened:
/// [`ProjectorQualificationVerifier::trusted_native`] proves every claim by
/// calling [`LiveCheckerReceipt::verify_current`] against a real, owner-issued,
/// revocation-aware receipt. A caller with no real checker owner cannot
/// construct a valid one except through [`bootstrap_native_source`], whose
/// own mechanical check is the only fact it may certify.
#[derive(Clone, Debug)]
pub struct NativeSourceEvidence {
    /// Exact implementation identity.
    pub code: ContentId,
    /// Exact transitive runtime dependency identity.
    pub dependencies: ContentId,
    /// The live, owner-issued receipt proving the required review occurred.
    pub receipt: LiveCheckerReceipt,
}

/// Evidence needed to admit a closed wasm projector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClosedWasmEvidence {
    /// Exact semantic module identity.
    pub module: ContentId,
    /// Complete imports discovered from the module.
    pub imports: DeterministicImportManifest,
    /// Exact qualified runtime.
    pub runtime: QualifiedRuntime,
    /// Admission evidence identity.
    pub admission: ContentId,
    /// Import discovery covered the complete transitive module.
    pub import_manifest_complete: bool,
    /// Start behavior was checked before instantiation.
    pub start_behavior_checked: bool,
    /// Fuel and memory limits are enforced by the runtime.
    pub budgets_enforced: bool,
}

/// Distinct projector-admission refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum QualificationError {
    /// Reviewed and loaded native code identities differ.
    NativeCodeMismatch,
    /// The supplied receipt is not for the required native-source-review scope.
    WrongCheckerScope,
    /// The supplied receipt's subject does not match the declared code identity.
    WrongCheckerSubject,
    /// The receipt's evidence grade is below the minimum this route requires.
    InsufficientEvidenceGrade,
    /// The receipt is no longer current, or its issuing owner is unreachable.
    CheckerUnavailable(String),
    /// Constructing the bootstrap checker binding or invocation failed.
    Checker(String),
    /// Wasm import discovery was incomplete.
    IncompleteImportManifest,
    /// Actual and policy wasm imports differ.
    ImportManifestMismatch,
    /// An ambient or nondeterministic wasm import was requested.
    ForbiddenImport(String),
    /// Wasm start behavior was not qualified.
    StartBehaviorUnchecked,
    /// Runtime numeric, ordering, or fresh-instance semantics are incomplete.
    RuntimeSemanticsUnqualified,
    /// Runtime fuel or memory bounds are absent.
    RuntimeBudgetsUnenforced,
    /// Canonical policy identity failed.
    CanonicalPolicy(String),
}

impl fmt::Display for QualificationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for QualificationError {}

/// Verifies the two and only two projector admission routes.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProjectorQualificationVerifier;

impl ProjectorQualificationVerifier {
    /// Qualifies exact trusted native code from a live, receipt-backed claim.
    ///
    /// # Errors
    /// Refuses unless `evidence.receipt` is currently valid under `authority`,
    /// scoped to [`native_source_review_scope`], and its checked subject
    /// matches `evidence.code`. This never trusts a self-reported boolean.
    pub fn trusted_native(
        policy: &ProjectorPolicy,
        evidence: NativeSourceEvidence,
        authority: &LiveCheckerAuthority,
    ) -> Result<ProjectorQualification, QualificationError> {
        evidence
            .receipt
            .verify_current(authority)
            .map_err(|error| QualificationError::CheckerUnavailable(error.to_string()))?;
        let receipt = evidence.receipt.receipt();
        if receipt.scope() != &native_source_review_scope()? {
            return Err(QualificationError::WrongCheckerScope);
        }
        if receipt.subject() != &content_id_subject(&evidence.code)? {
            return Err(QualificationError::WrongCheckerSubject);
        }
        if receipt.grade() < EvidenceGrade::Bootstrap {
            return Err(QualificationError::InsufficientEvidenceGrade);
        }
        Ok(ProjectorQualification::TrustedNative {
            source: QualifiedSourceClosure {
                code: evidence.code,
                dependencies: evidence.dependencies,
                review: receipt.id().content_id().clone(),
            },
            policy: policy_id(policy)?,
        })
    }

    /// Qualifies a closed wasm module and deterministic runtime.
    pub fn closed_wasm(
        policy: &ProjectorPolicy,
        evidence: ClosedWasmEvidence,
    ) -> Result<ProjectorQualification, QualificationError> {
        if !evidence.import_manifest_complete {
            return Err(QualificationError::IncompleteImportManifest);
        }
        if evidence.imports != policy.imports {
            return Err(QualificationError::ImportManifestMismatch);
        }
        for import in &evidence.imports.imports {
            if is_forbidden_import(import) {
                return Err(QualificationError::ForbiddenImport(import.clone()));
            }
        }
        if !evidence.start_behavior_checked {
            return Err(QualificationError::StartBehaviorUnchecked);
        }
        let semantics = &evidence.runtime.semantics;
        if !semantics.canonical_nan || !semantics.canonical_collections || !semantics.fresh_instance
        {
            return Err(QualificationError::RuntimeSemanticsUnqualified);
        }
        if !evidence.budgets_enforced {
            return Err(QualificationError::RuntimeBudgetsUnenforced);
        }
        Ok(ProjectorQualification::ClosedWasm {
            module: evidence.module,
            policy: policy_id(policy)?,
            runtime: evidence.runtime,
            imports: evidence.imports,
            admission: evidence.admission,
        })
    }
}

/// The fixed checker scope every native-source-review receipt must carry.
///
/// # Errors
/// Returns an error only if the fixed scope text itself cannot be canonicalized.
pub fn native_source_review_scope() -> Result<CheckScopeId, QualificationError> {
    CheckScopeId::from_text("projection/native-source-review-v1").map_err(checker_error)
}

/// Boots a fresh checker owner and issues the only kind of native-source
/// receipt available before NV12.06's full external-review checker chain
/// exists: proof that the code about to execute is exactly the code that
/// was declared, at [`EvidenceGrade::Bootstrap`], carrying no cross-world
/// reuse authority.
///
/// This mechanically checks exactly one fact -- `declared_code == loaded_code`
/// -- and certifies nothing about hidden state, ambient I/O, or FFI. A
/// caller needing those stronger claims must obtain them from a real,
/// externally owned checker instead; this route can never produce them.
///
/// The returned [`LiveCheckerOwner`] must be kept alive for as long as the
/// returned [`LiveCheckerAuthority`]/[`LiveCheckerReceipt`] pair is expected
/// to verify current: both are weak handles into it.
///
/// # Errors
/// Refuses if `declared_code != loaded_code`, or if constructing the
/// underlying checker binding, invocation, or receipt fails.
pub fn bootstrap_native_source(
    owner: OwnerBindingId,
    declared_code: ContentId,
    loaded_code: &ContentId,
) -> Result<(LiveCheckerOwner, LiveCheckerAuthority, LiveCheckerReceipt), QualificationError> {
    if &declared_code != loaded_code {
        return Err(QualificationError::NativeCodeMismatch);
    }
    let scope = native_source_review_scope()?;
    let template = CheckTemplate::new(
        "sim_incremental_core::projection::admission::bootstrap_native_source".to_owned(),
        vec![
            CheckArgument::BindingSlot,
            CheckArgument::SubjectSlot,
            CheckArgument::ScopeSlot,
        ],
        WorkingDirectoryPolicyId::from_text("projection/bootstrap-cwd-v1")
            .map_err(checker_error)?,
        EnvironmentPolicyId::from_text("projection/bootstrap-env-v1").map_err(checker_error)?,
        OutputShapeId::from_text("projection/bootstrap-result-v1").map_err(checker_error)?,
    )
    .map_err(checker_error)?;
    let binding = CheckerBinding::new(
        "projection/bootstrap-native-source".to_owned(),
        owner,
        "bootstrap_native_source".to_owned(),
        vec![ConformancePackId::from_text("projection/bootstrap-v1").map_err(checker_error)?],
        OutputShapeId::from_text("projection/bootstrap-receipt-v1").map_err(checker_error)?,
        RevocationSourceId::from_text("projection/bootstrap-revocation-v1")
            .map_err(checker_error)?,
        CommandId::from_text("projection/bootstrap-validation-v1").map_err(checker_error)?,
        CommandId::from_text("projection/bootstrap-docs-v1").map_err(checker_error)?,
        [scope.clone()].into_iter().collect(),
        template,
    )
    .map_err(checker_error)?;

    let subject = content_id_subject(&declared_code)?;
    let invocation = binding
        .instantiate(
            ProofCodeId::from_text(&hex(&declared_code)).map_err(checker_error)?,
            ConformancePackId::from_text("projection/bootstrap-v1").map_err(checker_error)?,
            subject,
            scope,
            CheckInputClosureId::from_text(&hex(loaded_code)).map_err(checker_error)?,
        )
        .map_err(checker_error)?;

    let generation = content_id_datum(&declared_code)
        .content_id()
        .map_err(|error| QualificationError::CanonicalPolicy(error.to_string()))?;
    let policy = PolicyId::from_text("projection/bootstrap-policy-v1").map_err(checker_error)?;
    let (checker_owner, issuer) = LiveCheckerOwner::boot(generation, binding, policy);
    checker_owner
        .mark_current(&invocation)
        .map_err(live_error)?;
    let authority = checker_owner.authority();
    let receipt = issuer
        .issue(
            invocation,
            CheckerResultId::from_text("projection/bootstrap-passed-v1").map_err(checker_error)?,
            EvidenceGrade::Bootstrap,
            EvidenceProvenanceId::from_text("projection/bootstrap-provenance-v1")
                .map_err(checker_error)?,
            EvidenceSetId::from_text("projection/bootstrap-support-v1").map_err(checker_error)?,
        )
        .map_err(live_error)?;
    Ok((checker_owner, authority, receipt))
}

fn content_id_subject(id: &ContentId) -> Result<CheckedSubjectId, QualificationError> {
    CheckedSubjectId::from_text(&hex(id)).map_err(checker_error)
}

fn hex(id: &ContentId) -> String {
    let mut out = String::with_capacity(id.bytes.len() * 2);
    for byte in id.bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn checker_error(error: sim_conformance_core::ConformanceError) -> QualificationError {
    QualificationError::Checker(error.to_string())
}

fn live_error(error: sim_conformance_core::LiveCheckerError) -> QualificationError {
    QualificationError::CheckerUnavailable(error.to_string())
}

pub(crate) fn policy_id(policy: &ProjectorPolicy) -> Result<ContentId, QualificationError> {
    let input_facts = policy
        .reads
        .facts()
        .map(|fact| Datum::String(fact.as_str().to_owned()))
        .collect();
    let imports = policy
        .imports
        .imports
        .iter()
        .cloned()
        .map(Datum::String)
        .collect();
    let fields = vec![
        (
            Symbol::new("input-shape"),
            content_id_datum(&policy.input_shape),
        ),
        (Symbol::new("reads"), Datum::Vector(input_facts)),
        (Symbol::new("imports"), Datum::Vector(imports)),
        (
            Symbol::new("execution"),
            Datum::Node {
                tag: Symbol::qualified("projection", "execution-semantics-v1"),
                fields: vec![
                    (
                        Symbol::new("id"),
                        Datum::String(policy.execution.id.clone()),
                    ),
                    (
                        Symbol::new("canonical-nan"),
                        Datum::Bool(policy.execution.canonical_nan),
                    ),
                    (
                        Symbol::new("canonical-collections"),
                        Datum::Bool(policy.execution.canonical_collections),
                    ),
                    (
                        Symbol::new("fresh-instance"),
                        Datum::Bool(policy.execution.fresh_instance),
                    ),
                ],
            },
        ),
        (
            Symbol::new("max-inputs"),
            number_datum(policy.budgets.max_inputs as u64),
        ),
        (
            Symbol::new("max-output-bytes"),
            number_datum(policy.budgets.max_output_bytes as u64),
        ),
        (
            Symbol::new("max-fuel"),
            number_datum(policy.budgets.max_fuel),
        ),
        (
            Symbol::new("max-memory-bytes"),
            number_datum(policy.budgets.max_memory_bytes as u64),
        ),
        (
            Symbol::new("requires-confinement"),
            Datum::Bool(policy.requires_confinement),
        ),
    ];
    Datum::Node {
        tag: Symbol::qualified("projection", "projector-policy-v1"),
        fields,
    }
    .content_id()
    .map_err(|error| QualificationError::CanonicalPolicy(error.to_string()))
}

pub(crate) fn content_id_datum(id: &ContentId) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("core", "content-id-v1"),
        fields: vec![
            (
                Symbol::new("algorithm"),
                Datum::Symbol(id.algorithm.clone()),
            ),
            (Symbol::new("bytes"), Datum::Bytes(id.bytes.to_vec())),
        ],
    }
}

fn number_datum(value: u64) -> Datum {
    Datum::Number(NumberLiteral {
        domain: Symbol::qualified("projection", "u64"),
        canonical: value.to_string(),
    })
}

fn is_forbidden_import(import: &str) -> bool {
    const FORBIDDEN: &[&str] = &[
        "wasi",
        "filesystem",
        "path_",
        "proc",
        "environment",
        "environ",
        "clock",
        "time",
        "random",
        "network",
        "socket",
        "thread",
        "shared-memory",
    ];
    let lower = import.to_ascii_lowercase();
    FORBIDDEN.iter().any(|needle| lower.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::{DeclaredInputSelector, ExecutionSemantics, ProjectionBudget};

    fn owner() -> OwnerBindingId {
        OwnerBindingId::from_text("projection/test-owner").unwrap()
    }

    fn code(seed: u8) -> ContentId {
        ContentId::from_bytes(Symbol::qualified("core", "sha256"), [seed; 32])
    }

    fn sample_policy() -> ProjectorPolicy {
        ProjectorPolicy {
            input_shape: code(200),
            reads: DeclaredInputSelector::new([]),
            imports: DeterministicImportManifest::default(),
            execution: ExecutionSemantics {
                id: "projection/native-v1".to_owned(),
                canonical_nan: true,
                canonical_collections: true,
                fresh_instance: true,
            },
            budgets: ProjectionBudget {
                max_inputs: 16,
                max_output_bytes: 4096,
                max_fuel: 1_000_000,
                max_memory_bytes: 1024 * 1024,
            },
            requires_confinement: false,
        }
    }

    #[test]
    fn bootstrap_native_source_refuses_a_declared_code_mismatch() {
        let result = bootstrap_native_source(owner(), code(1), &code(2));
        assert_eq!(result.unwrap_err(), QualificationError::NativeCodeMismatch);
    }

    #[test]
    fn bootstrap_native_source_issues_a_current_receipt_for_matching_code() {
        let (_checker_owner, authority, receipt) =
            bootstrap_native_source(owner(), code(1), &code(1)).unwrap();
        assert!(receipt.verify_current(&authority).is_ok());
        assert_eq!(receipt.receipt().grade(), EvidenceGrade::Bootstrap);
    }

    #[test]
    fn trusted_native_refuses_once_the_checker_owner_is_dropped() {
        let policy = sample_policy();
        let (checker_owner, authority, receipt) =
            bootstrap_native_source(owner(), code(1), &code(1)).unwrap();
        drop(checker_owner);
        let evidence = NativeSourceEvidence {
            code: code(1),
            dependencies: code(3),
            receipt,
        };
        let result = ProjectorQualificationVerifier::trusted_native(&policy, evidence, &authority);
        assert!(matches!(
            result,
            Err(QualificationError::CheckerUnavailable(_))
        ));
    }

    #[test]
    fn trusted_native_refuses_a_receipt_whose_subject_does_not_match_the_declared_code() {
        let policy = sample_policy();
        let (checker_owner, authority, receipt) =
            bootstrap_native_source(owner(), code(1), &code(1)).unwrap();
        let evidence = NativeSourceEvidence {
            code: code(9),
            dependencies: code(3),
            receipt,
        };
        let result = ProjectorQualificationVerifier::trusted_native(&policy, evidence, &authority);
        assert_eq!(result.unwrap_err(), QualificationError::WrongCheckerSubject);
        drop(checker_owner);
    }

    #[test]
    fn trusted_native_admits_a_genuinely_current_bootstrap_receipt() {
        let policy = sample_policy();
        let (checker_owner, authority, receipt) =
            bootstrap_native_source(owner(), code(1), &code(1)).unwrap();
        let evidence = NativeSourceEvidence {
            code: code(1),
            dependencies: code(3),
            receipt,
        };
        let qualification =
            ProjectorQualificationVerifier::trusted_native(&policy, evidence, &authority).unwrap();
        assert!(matches!(
            qualification,
            ProjectorQualification::TrustedNative { .. }
        ));
        drop(checker_owner);
    }
}
