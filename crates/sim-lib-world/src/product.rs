// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use sim_incremental_core::projection::{
    ConclusionId, DeclaredInputSelector, DeterministicImportManifest, ExecutionSemantics,
    Explanation, FactId, FederatedClosure, ObservedFact, ObservedWorld, OwnerProjectionGraph,
    PackageIdentity, ProjectionBudget, ProjectionDigest, ProjectionEngine, ProjectionError,
    ProjectionKindRef, ProjectionRegistry, ProjectionResult, ProjectionSpec, ProjectorPolicy,
    install_baseline_providers,
};
use sim_kernel::{Datum, Symbol};

use crate::provider::{PROVIDER_SOURCE, WorldConfigShape, config, config_symbol, content_id};

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
    pairs: Vec<(String, String)>,
}

/// One caller declaration admitted into a product closure.
///
/// The kind names a loaded projector. The fact, owner, and conclusion are
/// the caller's own identities. Nothing here matches those strings.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Declaration {
    /// Loaded projection kind.
    pub kind: String,
    /// Fact the conclusion consumes.
    pub fact: String,
    /// Owner that declares the conclusion.
    pub owner: String,
    /// Conclusion that consumes the fact.
    pub conclusion: String,
}

/// Pure read-only product for semantic projection, diff, and explanation.
#[derive(Clone)]
pub struct WorldProduct {
    state: Arc<WorldState>,
}

impl WorldProduct {
    /// Constructs the bundled baseline provider from its exact authored source.
    pub fn bundled() -> Result<Self, WorldError> {
        let (registry, shape, package) = open_registry()?;
        let source = FactId::new(SOURCE_FACT)?;
        let disclosure = FactId::new(DISCLOSURE_FACT)?;
        let proofs = crate::fixture::proofs();
        let Some(first) = proofs.first() else {
            return Err(WorldError::Qualification(
                "caller-named proofs are empty".to_owned(),
            ));
        };
        if proofs
            .iter()
            .any(|proof| proof.owner != first.owner || proof.conclusion != first.conclusion)
        {
            return Err(WorldError::Qualification(
                "caller-named proofs name more than one owner".to_owned(),
            ));
        }
        let mut universe = vec![source.clone(), disclosure.clone()];
        let mut named_facts = BTreeSet::new();
        let mut pairs = vec![
            (SOURCE_KIND.to_owned(), SOURCE_FACT.to_owned()),
            (DISCLOSURE_KIND.to_owned(), DISCLOSURE_FACT.to_owned()),
        ];
        for proof in proofs {
            let fact = FactId::new(proof.fact)?;
            universe.push(fact.clone());
            named_facts.insert(fact);
            pairs.push((proof.kind.to_owned(), proof.fact.to_owned()));
        }
        let closure = FederatedClosure::seal(
            universe,
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
                OwnerProjectionGraph::new(
                    first.owner,
                    [(ConclusionId::new(first.conclusion)?, named_facts)],
                )
                .map_err(|error| WorldError::Qualification(error.to_string()))?,
            ],
        )
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
        Ok(Self::from_parts(registry, shape, closure, package, pairs))
    }

    /// Admits `declarations` through the same loaded projectors as [`Self::bundled`].
    ///
    /// # Errors
    /// Returns an error when a declaration is empty, an identity is rejected,
    /// or two owners claim one conclusion.
    pub fn declared(declarations: Vec<Declaration>) -> Result<Self, WorldError> {
        if declarations.is_empty() {
            return Err(WorldError::Qualification(
                "caller declarations are empty".to_owned(),
            ));
        }
        let (registry, shape, package) = open_registry()?;
        let mut universe = Vec::new();
        let mut owners: BTreeMap<String, BTreeMap<String, BTreeSet<FactId>>> = BTreeMap::new();
        let mut pairs = Vec::with_capacity(declarations.len());
        for declaration in &declarations {
            if declaration.kind.is_empty()
                || declaration.fact.is_empty()
                || declaration.owner.is_empty()
                || declaration.conclusion.is_empty()
            {
                return Err(WorldError::Qualification(
                    "caller declaration has an empty field".to_owned(),
                ));
            }
            let fact = FactId::new(&declaration.fact)?;
            universe.push(fact.clone());
            owners
                .entry(declaration.owner.clone())
                .or_default()
                .entry(declaration.conclusion.clone())
                .or_default()
                .insert(fact);
            pairs.push((declaration.kind.clone(), declaration.fact.clone()));
        }
        let mut graphs = Vec::with_capacity(owners.len());
        for (owner, conclusions) in owners {
            let mapped = conclusions
                .into_iter()
                .map(|(conclusion, facts)| {
                    Ok((
                        ConclusionId::new(conclusion)
                            .map_err(|error| WorldError::Qualification(error.to_string()))?,
                        facts,
                    ))
                })
                .collect::<Result<Vec<_>, WorldError>>()?;
            graphs.push(
                OwnerProjectionGraph::new(&owner, mapped)
                    .map_err(|error| WorldError::Qualification(error.to_string()))?,
            );
        }
        let closure = FederatedClosure::seal(universe, graphs)
            .map_err(|error| WorldError::Qualification(error.to_string()))?;
        Ok(Self::from_parts(registry, shape, closure, package, pairs))
    }

    fn from_parts(
        registry: ProjectionRegistry,
        shape: WorldConfigShape,
        closure: FederatedClosure,
        package: PackageIdentity,
        pairs: Vec<(String, String)>,
    ) -> Self {
        Self {
            state: Arc::new(WorldState {
                registry,
                shape,
                closure,
                package,
                pairs,
            }),
        }
    }

    /// Projects one caller-supplied observed semantic fact.
    pub fn project(
        &self,
        kind: &str,
        fact: &str,
        semantic: Datum,
        envelope: Option<Datum>,
    ) -> Result<WorldProjection, WorldError> {
        self.ensure_pair(kind, fact)?;
        let fact_id = FactId::new(fact)?;
        let world = ObservedWorld::new([(fact_id.clone(), ObservedFact { semantic, envelope })])?;
        let policy = self.policy(fact_id);
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
        // Qualification is fetched internally by `project`, from this exact
        // registry, at the moment of dispatch -- not obtained here and
        // passed in, which would make it a replayable bearer credential.
        let result =
            ProjectionEngine::new(&self.state.registry, &self.state.shape, &self.state.closure)
                .project(&world, &spec, &policy, None)?;
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
        Ok(explanation_value(explanation))
    }

    /// Reports effect invocations owned by this product; the value is always zero.
    #[must_use]
    pub const fn effect_calls(&self) -> usize {
        0
    }

    fn ensure_pair(&self, kind: &str, fact: &str) -> Result<(), WorldError> {
        if self
            .state
            .pairs
            .iter()
            .any(|(admitted_kind, admitted_fact)| admitted_kind == kind && admitted_fact == fact)
        {
            Ok(())
        } else {
            Err(WorldError::UnsupportedPair {
                kind: kind.to_owned(),
                fact: fact.to_owned(),
            })
        }
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

pub(crate) fn explanation_value(explanation: Explanation) -> Datum {
    Datum::Node {
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
    }
}

fn open_registry() -> Result<(ProjectionRegistry, WorldConfigShape, PackageIdentity), WorldError> {
    let code = content_id(Datum::String(PROVIDER_SOURCE.to_owned()))?;
    let shape_id = content_id(Datum::String("shape/world-config-v1".to_owned()))?;
    let requested = PackageIdentity {
        name: env!("CARGO_PKG_NAME").to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        code,
    };
    let mut registry = ProjectionRegistry::new();
    // The registered identity's `code` is computed inside sim-incremental-core
    // from the compiled provider source. Later specs must use that identity.
    let package = install_baseline_providers(&mut registry, shape_id.clone(), requested)?;
    Ok((registry, WorldConfigShape { id: shape_id }, package))
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
            (
                Symbol::new("shape"),
                Datum::String(config_symbol().as_qualified_str()),
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
