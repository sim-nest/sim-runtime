use std::{error::Error, fmt};

use sim_kernel::{ContentId, Datum, NumberLiteral, Symbol};

use super::{
    DeterministicImportManifest, ProjectorPolicy, ProjectorQualification, QualifiedRuntime,
    QualifiedSourceClosure,
};

/// Evidence needed to trust an exact native projector implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeSourceEvidence {
    /// Exact implementation identity.
    pub code: ContentId,
    /// Exact transitive runtime dependency identity.
    pub dependencies: ContentId,
    /// Independent review evidence identity.
    pub review: ContentId,
    /// Source and dependencies were both reviewed.
    pub source_and_dependencies_reviewed: bool,
    /// Deny-ambient-I/O review found no unexplained path.
    pub ambient_io_closed: bool,
    /// FFI, unsafe, globals, mutable caches, and nondeterminism were reviewed.
    pub hidden_state_reviewed: bool,
    /// The inspected code identity equals the code that will be loaded.
    pub loaded_code_matches: bool,
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
    /// Native code did not pass exact source and dependency review.
    NativeSourceReviewMissing,
    /// Native code retained an unexplained ambient or hidden input.
    NativeAmbientInput,
    /// Reviewed and loaded native code identities differ.
    NativeCodeMismatch,
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
    /// Qualifies exact trusted native code.
    pub fn trusted_native(
        policy: &ProjectorPolicy,
        evidence: NativeSourceEvidence,
    ) -> Result<ProjectorQualification, QualificationError> {
        if !evidence.source_and_dependencies_reviewed {
            return Err(QualificationError::NativeSourceReviewMissing);
        }
        if !evidence.ambient_io_closed || !evidence.hidden_state_reviewed {
            return Err(QualificationError::NativeAmbientInput);
        }
        if !evidence.loaded_code_matches {
            return Err(QualificationError::NativeCodeMismatch);
        }
        Ok(ProjectorQualification::TrustedNative {
            source: QualifiedSourceClosure {
                code: evidence.code,
                dependencies: evidence.dependencies,
                review: evidence.review,
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
