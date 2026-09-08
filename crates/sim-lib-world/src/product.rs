use std::{collections::BTreeSet, sync::Arc};

use sim_incremental_core::projection::{
    ConclusionId, DeclaredInputSelector, DeterministicImportManifest, ExecutionSemantics, FactId,
    FederatedClosure, NativeSourceEvidence, ObservedFact, ObservedWorld, OwnerProjectionGraph,
    PackageIdentity, ProjectionBudget, ProjectionDigest, ProjectionEngine, ProjectionError,
    ProjectionKindRef, ProjectionRegistry, ProjectionResult, ProjectionSpec, ProjectorPolicy,
    ProjectorQualificationVerifier, install_baseline_providers,
};
use sim_kernel::{ContentId, Datum, Symbol};

use crate::provider::{PROVIDER_SOURCE, WorldConfigShape, config, content_id};

/// Semantic source fact used by the bundled reference world.
pub const SOURCE_FACT: &str = "source/public-api";
/// Audit-derived disclosure-policy fact used by the bundled reference world.
pub const DISCLOSURE_FACT: &str = "policy/no-v3-disclosure";
/// Source-only conclusion used to prove projection-scoped invalidation.
pub const SOURCE_CONCLUSION: &str = "conclusion/source-api";
/// Public-release conclusion which consumes the disclosure policy.
pub const DISCLOSURE_CONCLUSION: &str = "conclusion/public-release";

const SOURCE_KIND: &str = "world/public-api-v1";
const DISCLOSURE_KIND: &str = "no-v3/disclosure-policy-v1";

/// A checked projection together with its stable SIM record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldProjection {
    /// Complete qualified core result.
    pub result: ProjectionResult,
    /// Stable product record suitable for expression-codec rendering.
    pub value: Datum,
}

/// Product-level construction or projection failure.
#[derive(Debug)]
pub enum WorldError {
    /// Projection core rejected the request.
    Projection(ProjectionError),
    /// Qualification evidence did not close.
    Qualification(String),
    /// The requested reference kind/fact pair is not admitted.
    UnsupportedPair {
        /// Requested provider kind.
        kind: String,
        /// Requested fact identity.
        fact: String,
    },
}

impl std::fmt::Display for WorldError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for WorldError {}

impl From<ProjectionError> for WorldError {
    fn from(value: ProjectionError) -> Self {
        Self::Projection(value)
    }
}

struct WorldState {
    registry: ProjectionRegistry,
    shape: WorldConfigShape,
    closure: FederatedClosure,
    package: PackageIdentity,
}

/// Pure read-only product for semantic projection, diff, and explanation.
#[derive(Clone)]
pub struct WorldProduct {
    state: Arc<WorldState>,
}

impl WorldProduct {
    /// Constructs the bundled baseline provider from its exact authored source.
    pub fn bundled() -> Result<Self, WorldError> {
        let code = content_id(Datum::String(PROVIDER_SOURCE.to_owned()))?;
        let shape_id = content_id(Datum::String("shape/world-config-v1".to_owned()))?;
        let package = PackageIdentity {
            name: env!("CARGO_PKG_NAME").to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
            code,
        };
        let mut registry = ProjectionRegistry::new();
        install_baseline_providers(&mut registry, shape_id.clone(), package.clone())?;
        let source = FactId::new(SOURCE_FACT)?;
        let disclosure = FactId::new(DISCLOSURE_FACT)?;
        let closure = FederatedClosure::seal(
            [source.clone(), disclosure.clone()],
            [
                OwnerProjectionGraph::new(
                    "sim-runtime/sim-lib-world",
                    [(
                        ConclusionId::new(SOURCE_CONCLUSION)?,
                        BTreeSet::from([source.clone()]),
                    )],
                )
                .map_err(|error| WorldError::Qualification(error.to_string()))?,
                OwnerProjectionGraph::new(
                    "sim-private/disclosure",
                    [(
                        ConclusionId::new(DISCLOSURE_CONCLUSION)?,
                        BTreeSet::from([source, disclosure]),
                    )],
                )
                .map_err(|error| WorldError::Qualification(error.to_string()))?,
            ],
        )
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
        Ok(Self {
            state: Arc::new(WorldState {
                registry,
                shape: WorldConfigShape { id: shape_id },
                closure,
                package,
            }),
        })
    }

    /// Projects one caller-supplied observed semantic fact.
    pub fn project(
        &self,
        kind: &str,
        fact: &str,
        semantic: Datum,
        envelope: Option<Datum>,
    ) -> Result<WorldProjection, WorldError> {
        ensure_pair(kind, fact)?;
        let fact_id = FactId::new(fact)?;
        let world = ObservedWorld::new([(fact_id.clone(), ObservedFact { semantic, envelope })])?;
        let policy = self.policy(fact_id);
        let qualification = ProjectorQualificationVerifier::trusted_native(
            &policy,
            NativeSourceEvidence {
                code: self.state.package.code.clone(),
                dependencies: dependency_closure_id()?,
                review: review_id()?,
                source_and_dependencies_reviewed: true,
                ambient_io_closed: true,
                hidden_state_reviewed: true,
                loaded_code_matches: true,
            },
        )
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
        let spec = ProjectionSpec {
            id: content_id(Datum::Node {
                tag: Symbol::qualified("world", "projection-request-v1"),
                fields: vec![
                    (Symbol::new("kind"), Datum::String(kind.to_owned())),
                    (Symbol::new("fact"), Datum::String(fact.to_owned())),
                ],
            })?,
            kind: ProjectionKindRef::new(kind)?,
            config: config(),
            config_shape: self.state.shape.id.clone(),
            provider: self.state.package.clone(),
        };
        let result =
            ProjectionEngine::new(&self.state.registry, &self.state.shape, &self.state.closure)
                .project(&world, &spec, &policy, Some(&qualification), None)?;
        let value = projection_value(kind, fact, &result);
        Ok(WorldProjection { result, value })
    }

    /// Compares two supplied semantic values under one qualified projection.
    pub fn diff(
        &self,
        kind: &str,
        fact: &str,
        before: Datum,
        after: Datum,
    ) -> Result<Datum, WorldError> {
        let before = self.project(kind, fact, before, None)?;
        let after = self.project(kind, fact, after, None)?;
        let changed = before.result.digest != after.result.digest;
        let affected = if changed {
            after
                .result
                .affected
                .iter()
                .map(|id| Datum::String(id.as_str().to_owned()))
                .collect()
        } else {
            Vec::new()
        };
        Ok(Datum::Node {
            tag: Symbol::qualified("world", "diff-v1"),
            fields: vec![
                (Symbol::new("before"), digest_datum(&before.result.digest)),
                (Symbol::new("after"), digest_datum(&after.result.digest)),
                (Symbol::new("changed"), Datum::Bool(changed)),
                (Symbol::new("affected"), Datum::Vector(affected)),
            ],
        })
    }

    /// Explains one exact owner/conclusion/fact path from the sealed closure.
    pub fn why(&self, conclusion: &str, fact: &str) -> Result<Datum, WorldError> {
        let explanation = self
            .state
            .closure
            .explain(&ConclusionId::new(conclusion)?, &FactId::new(fact)?)
            .map_err(|error| WorldError::Qualification(error.to_string()))?;
        Ok(Datum::Node {
            tag: Symbol::qualified("world", "explanation-v1"),
            fields: vec![
                (
                    Symbol::new("conclusion"),
                    Datum::String(explanation.conclusion.as_str().to_owned()),
                ),
                (
                    Symbol::new("fact"),
                    Datum::String(explanation.fact.as_str().to_owned()),
                ),
                (
                    Symbol::new("path"),
                    Datum::Vector(explanation.path.into_iter().map(Datum::String).collect()),
                ),
            ],
        })
    }

    /// Reports effect invocations owned by this product; the value is always zero.
    #[must_use]
    pub const fn effect_calls(&self) -> usize {
        0
    }

    fn policy(&self, fact: FactId) -> ProjectorPolicy {
        ProjectorPolicy {
            input_shape: self.state.shape.id.clone(),
            reads: DeclaredInputSelector::new([fact]),
            imports: DeterministicImportManifest::default(),
            execution: ExecutionSemantics {
                id: "projection/trusted-native-v1".to_owned(),
                canonical_nan: true,
                canonical_collections: true,
                fresh_instance: true,
            },
            budgets: ProjectionBudget {
                max_inputs: 1,
                max_output_bytes: 64 * 1024,
                max_fuel: 1_000_000,
                max_memory_bytes: 4 * 1024 * 1024,
            },
            requires_confinement: false,
        }
    }
}

fn ensure_pair(kind: &str, fact: &str) -> Result<(), WorldError> {
    if matches!(
        (kind, fact),
        (SOURCE_KIND, SOURCE_FACT) | (DISCLOSURE_KIND, DISCLOSURE_FACT)
    ) {
        Ok(())
    } else {
        Err(WorldError::UnsupportedPair {
            kind: kind.to_owned(),
            fact: fact.to_owned(),
        })
    }
}

fn dependency_closure_id() -> Result<ContentId, ProjectionError> {
    content_id(Datum::Node {
        tag: Symbol::qualified("world", "native-dependency-closure-v1"),
        fields: vec![
            (
                Symbol::new("sim-incremental-core"),
                Datum::String("0.5.0".to_owned()),
            ),
            (Symbol::new("sim-kernel"), Datum::String("0.4.0".to_owned())),
        ],
    })
}

fn review_id() -> Result<ContentId, ProjectionError> {
    content_id(Datum::Node {
        tag: Symbol::qualified("world", "native-review-v1"),
        fields: vec![
            (
                Symbol::new("source"),
                Datum::String(PROVIDER_SOURCE.to_owned()),
            ),
            (Symbol::new("io-ports"), Datum::Vector(Vec::new())),
            (Symbol::new("unsafe"), Datum::Bool(false)),
            (Symbol::new("mutable-state"), Datum::Bool(false)),
        ],
    })
}

fn projection_value(kind: &str, fact: &str, result: &ProjectionResult) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "projection-v1"),
        fields: vec![
            (Symbol::new("kind"), Datum::String(kind.to_owned())),
            (Symbol::new("fact"), Datum::String(fact.to_owned())),
            (Symbol::new("digest"), digest_datum(&result.digest)),
            (Symbol::new("value"), result.projection.clone()),
            (
                Symbol::new("affected"),
                Datum::Vector(
                    result
                        .affected
                        .iter()
                        .map(|id| Datum::String(id.as_str().to_owned()))
                        .collect(),
                ),
            ),
        ],
    }
}

fn digest_datum(digest: &ProjectionDigest) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("core", "content-id-v1"),
        fields: vec![
            (
                Symbol::new("algorithm"),
                Datum::String(digest.0.algorithm.to_string()),
            ),
            (Symbol::new("bytes"), Datum::String(hex(&digest.0.bytes))),
        ],
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}
