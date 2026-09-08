use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    sync::Mutex,
};

use sim_kernel::{ContentId, Datum};

macro_rules! string_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
        pub struct $name(String);

        impl $name {
            /// Constructs an identifier after rejecting an empty spelling.
            pub fn new(value: impl Into<String>) -> Result<Self, ProjectionError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(ProjectionError::InvalidIdentifier(stringify!($name)));
                }
                Ok(Self(value))
            }

            /// Returns the stable identifier spelling.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

string_id!(FactId, "Stable identity of one semantic observed fact.");
string_id!(
    ConclusionId,
    "Stable identity of a conclusion consuming facts."
);
string_id!(
    ProjectionKindRef,
    "Open identifier of a loaded projection kind."
);

/// Exact package and implementation identity of a projection provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageIdentity {
    /// Registry package name.
    pub name: String,
    /// Semantic version used by the provider.
    pub version: String,
    /// Content identity of the exact loaded implementation.
    pub code: ContentId,
}

/// One fact with a semantic value and a diagnostic envelope.
///
/// Only `semantic` enters projection identity. The envelope may carry timing,
/// retries, path aliases, broker location, or logs without invalidating reuse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObservedFact {
    /// Canonical value available to a projector.
    pub semantic: Datum,
    /// Diagnostic data retained outside semantic identity.
    pub envelope: Option<Datum>,
}

/// Immutable observed facts from which a selector creates a bounded view.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObservedWorld {
    facts: BTreeMap<FactId, ObservedFact>,
}

impl ObservedWorld {
    /// Builds a world and refuses duplicate fact identities.
    pub fn new(
        facts: impl IntoIterator<Item = (FactId, ObservedFact)>,
    ) -> Result<Self, ProjectionError> {
        let mut world = Self::default();
        for (id, fact) in facts {
            if world.facts.insert(id.clone(), fact).is_some() {
                return Err(ProjectionError::DuplicateFact(id));
            }
        }
        Ok(world)
    }

    pub(crate) fn select(
        &self,
        selector: &DeclaredInputSelector,
    ) -> Result<ProjectionInputs, ProjectionError> {
        let mut selected = BTreeMap::new();
        for id in &selector.facts {
            let fact = self
                .facts
                .get(id)
                .ok_or_else(|| ProjectionError::MissingFact(id.clone()))?;
            selected.insert(id.clone(), fact.semantic.clone());
        }
        Ok(ProjectionInputs {
            facts: selected,
            accessed: Mutex::new(BTreeSet::new()),
        })
    }
}

/// Closed set of facts admitted as one provider invocation's entire input.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeclaredInputSelector {
    facts: BTreeSet<FactId>,
}

impl DeclaredInputSelector {
    /// Builds a canonical selector.
    #[must_use]
    pub fn new(facts: impl IntoIterator<Item = FactId>) -> Self {
        Self {
            facts: facts.into_iter().collect(),
        }
    }

    /// Returns selected fact identities in canonical order.
    pub fn facts(&self) -> impl ExactSizeIterator<Item = &FactId> {
        self.facts.iter()
    }
}

/// Bounded immutable input view supplied to a projector.
///
/// Its fields are private so a provider cannot reach the rest of the world or
/// any diagnostic envelope. Every successful read is recorded.
#[derive(Debug)]
pub struct ProjectionInputs {
    facts: BTreeMap<FactId, Datum>,
    accessed: Mutex<BTreeSet<FactId>>,
}

impl ProjectionInputs {
    /// Reads a selected fact and records the access.
    pub fn get(&self, id: &FactId) -> Option<&Datum> {
        let value = self.facts.get(id)?;
        self.accessed
            .lock()
            .expect("projection access mutex poisoned")
            .insert(id.clone());
        Some(value)
    }

    /// Iterates over every selected fact and records each access.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&FactId, &Datum)> {
        self.accessed
            .lock()
            .expect("projection access mutex poisoned")
            .extend(self.facts.keys().cloned());
        self.facts.iter()
    }

    pub(crate) fn accessed(&self) -> BTreeSet<FactId> {
        self.accessed
            .lock()
            .expect("projection access mutex poisoned")
            .clone()
    }
}

/// Canonical provider output and its declared fact dependencies.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionOutput {
    /// Canonical semantic projection value.
    pub value: Datum,
    /// Exact facts on which the value depends.
    pub dependencies: BTreeSet<FactId>,
}

/// Loaded implementation of an open projection kind.
pub trait ProjectionProvider: Send + Sync {
    /// Open kind implemented by this provider.
    fn kind(&self) -> &ProjectionKindRef;
    /// Stable Shape identity used to check configuration before invocation.
    fn config_shape(&self) -> &ContentId;
    /// Projects solely over the bounded immutable input view.
    fn project(
        &self,
        inputs: &ProjectionInputs,
        config: &Datum,
    ) -> Result<ProjectionOutput, ProjectionError>;
}

/// Checked request for one loaded projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionSpec {
    /// Stable request identity.
    pub id: ContentId,
    /// Open loaded provider kind.
    pub kind: ProjectionKindRef,
    /// Provider-specific configuration value.
    pub config: Datum,
    /// Shape that must match the loaded provider's declared config Shape.
    pub config_shape: ContentId,
    /// Exact provider package and code identity.
    pub provider: PackageIdentity,
}

/// Fuel, memory, output, and selected-input ceilings for projection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionBudget {
    /// Maximum selected facts.
    pub max_inputs: usize,
    /// Maximum canonical output bytes.
    pub max_output_bytes: usize,
    /// Maximum wasm fuel, when the closed wasm route is used.
    pub max_fuel: u64,
    /// Maximum wasm linear-memory bytes.
    pub max_memory_bytes: usize,
}

/// Closed deterministic imports made available to a wasm projector.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DeterministicImportManifest {
    /// Fully qualified `module/name` imports in canonical order.
    pub imports: BTreeSet<String>,
}

/// Runtime semantics that affect deterministic projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExecutionSemantics {
    /// Stable semantics family and version.
    pub id: String,
    /// Whether canonical NaN behavior is fixed.
    pub canonical_nan: bool,
    /// Whether collection traversal order is fixed.
    pub canonical_collections: bool,
    /// Whether every invocation starts with fresh mutable instance state.
    pub fresh_instance: bool,
}

/// Admission policy bound before projector qualification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectorPolicy {
    /// Semantic identity of the input Shape.
    pub input_shape: ContentId,
    /// Complete selected input universe.
    pub reads: DeclaredInputSelector,
    /// Closed deterministic import universe.
    pub imports: DeterministicImportManifest,
    /// Deterministic runtime semantics.
    pub execution: ExecutionSemantics,
    /// Bounded work and output policy.
    pub budgets: ProjectionBudget,
    /// Whether effect confinement is independently required on this host.
    pub requires_confinement: bool,
}

/// Source closure admitted for a trusted native projector.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualifiedSourceClosure {
    /// Exact implementation content identity.
    pub code: ContentId,
    /// Exact transitive runtime dependency closure identity.
    pub dependencies: ContentId,
    /// Independent source/dependency review evidence identity.
    pub review: ContentId,
}

/// Runtime admitted for the closed wasm route.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QualifiedRuntime {
    /// Exact runtime implementation identity.
    pub code: ContentId,
    /// Qualified execution semantics.
    pub semantics: ExecutionSemantics,
}

/// One of the two projector admission routes allowed by the roadmap.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectorQualification {
    /// Exact reviewed native code and dependency closure.
    TrustedNative {
        /// Qualified source closure.
        source: QualifiedSourceClosure,
        /// Policy identity reviewed with that source.
        policy: ContentId,
    },
    /// Closed wasm module with verified imports and runtime behavior.
    ClosedWasm {
        /// Exact semantic module identity.
        module: ContentId,
        /// Policy identity used for admission.
        policy: ContentId,
        /// Qualified runtime and semantics.
        runtime: QualifiedRuntime,
        /// Verified complete import manifest.
        imports: DeterministicImportManifest,
        /// Admission evidence identity.
        admission: ContentId,
    },
}

impl ProjectorQualification {
    pub(crate) fn implementation(&self) -> &ContentId {
        match self {
            Self::TrustedNative { source, .. } => &source.code,
            Self::ClosedWasm { module, .. } => module,
        }
    }

    pub(crate) fn policy(&self) -> &ContentId {
        match self {
            Self::TrustedNative { policy, .. } | Self::ClosedWasm { policy, .. } => policy,
        }
    }
}

/// Independent bounded-effect confinement evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfinementEvidence {
    /// Membrane implementation identity.
    pub membrane: String,
    /// Exact bounded policy identity.
    pub policy: ContentId,
    /// Live host readiness was probed at dispatch.
    pub live: bool,
}

/// Exact facts read through the bounded projection input view.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediatedAccessWitness {
    /// Facts selected by policy.
    pub selected: BTreeSet<FactId>,
    /// Facts actually read by the provider.
    pub accessed: BTreeSet<FactId>,
}

/// One causal path from requested conclusion to changed leaf fact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Explanation {
    /// Requested conclusion.
    pub conclusion: ConclusionId,
    /// Changed fact on which it depends.
    pub fact: FactId,
    /// Ordered owner-local path including conclusion and fact endpoints.
    pub path: Vec<String>,
}

/// Durable semantic digest of one qualified projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionDigest(pub ContentId);

/// Complete checked result of one provider invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionResult {
    /// Canonical semantic projection.
    pub projection: Datum,
    /// Exact selected and accessed facts.
    pub mediated_access: MediatedAccessWitness,
    /// Qualification used for this invocation.
    pub projector_qualification: ProjectorQualification,
    /// Independent confinement evidence, when required.
    pub confinement: Option<ConfinementEvidence>,
    /// Durable semantic identity excluding diagnostic envelopes.
    pub digest: ProjectionDigest,
    /// Conclusions affected by the consumed facts.
    pub affected: Vec<ConclusionId>,
    /// Causal explanations for affected conclusion/fact pairs.
    pub explanations: Vec<Explanation>,
}

/// Fail-closed projection refusal with distinct policy boundaries.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectionError {
    /// An identifier was empty.
    InvalidIdentifier(&'static str),
    /// A world declared the same fact twice.
    DuplicateFact(FactId),
    /// A selected fact was absent.
    MissingFact(FactId),
    /// No loaded provider owns the requested open kind.
    UnknownProvider(ProjectionKindRef),
    /// More than one loaded provider claimed the same kind.
    DuplicateProvider(ProjectionKindRef),
    /// Loaded provider and requested config Shape disagree.
    ConfigShapeMismatch,
    /// Configuration did not satisfy its declared Shape.
    InvalidConfig(String),
    /// A path selector or logical path was not canonical or valid.
    InvalidPathSelection(String),
    /// Provider code identity differs from qualified loaded code.
    CodeIdentityMismatch,
    /// Projector qualification is missing or invalid.
    UnqualifiedProjector(String),
    /// Required confinement is absent or unavailable.
    UnavailableConfinement(String),
    /// Provider read and dependency declarations disagree.
    UndeclaredAccess {
        /// Fact read without a matching dependency claim.
        accessed: FactId,
    },
    /// Provider claimed a dependency it never read.
    UnreadDependency(FactId),
    /// A configured budget was exceeded.
    BudgetExceeded(&'static str),
    /// Canonical projection identity could not be constructed.
    Canonical(String),
    /// A requested explanation path does not exist.
    MissingExplanation {
        /// Requested conclusion.
        conclusion: ConclusionId,
        /// Requested leaf fact.
        fact: FactId,
    },
}

impl fmt::Display for ProjectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for ProjectionError {}
