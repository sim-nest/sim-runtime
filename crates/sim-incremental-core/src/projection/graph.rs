use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use super::{ConclusionId, Explanation, FactId};

/// One owner's canonical conclusion-to-fact dependency graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnerProjectionGraph {
    owner: String,
    dependencies: BTreeMap<ConclusionId, BTreeSet<FactId>>,
}

impl OwnerProjectionGraph {
    /// Constructs a local owner graph and rejects an empty owner or conclusion.
    pub fn new(
        owner: impl Into<String>,
        dependencies: impl IntoIterator<Item = (ConclusionId, BTreeSet<FactId>)>,
    ) -> Result<Self, ClosureError> {
        let owner = owner.into();
        if owner.trim().is_empty() {
            return Err(ClosureError::EmptyOwner);
        }
        let mut canonical = BTreeMap::new();
        for (conclusion, facts) in dependencies {
            if facts.is_empty() {
                return Err(ClosureError::ConclusionWithoutFacts(conclusion));
            }
            if canonical.insert(conclusion.clone(), facts).is_some() {
                return Err(ClosureError::DuplicateConclusion(conclusion));
            }
        }
        if canonical.is_empty() {
            return Err(ClosureError::EmptyOwnerGraph(owner));
        }
        Ok(Self {
            owner,
            dependencies: canonical,
        })
    }

    /// Returns the owner identity.
    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }
}

/// Sealed union of local owner graphs with a canonical reverse dependency map.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FederatedClosure {
    owners: BTreeMap<ConclusionId, String>,
    dependencies: BTreeMap<ConclusionId, BTreeSet<FactId>>,
    consumers: BTreeMap<FactId, BTreeSet<ConclusionId>>,
}

impl FederatedClosure {
    /// Joins owner-local graphs and refuses unknown facts or duplicate ownership.
    pub fn seal(
        facts: impl IntoIterator<Item = FactId>,
        graphs: impl IntoIterator<Item = OwnerProjectionGraph>,
    ) -> Result<Self, ClosureError> {
        let facts = facts.into_iter().collect::<BTreeSet<_>>();
        let mut owners = BTreeMap::new();
        let mut dependencies = BTreeMap::new();
        let mut consumers = facts
            .iter()
            .cloned()
            .map(|fact| (fact, BTreeSet::new()))
            .collect::<BTreeMap<_, _>>();
        for graph in graphs {
            for (conclusion, required) in graph.dependencies {
                if let Some(first) = owners.insert(conclusion.clone(), graph.owner.clone()) {
                    return Err(ClosureError::DuplicateOwner {
                        conclusion,
                        first,
                        second: graph.owner,
                    });
                }
                for fact in &required {
                    let Some(fact_consumers) = consumers.get_mut(fact) else {
                        return Err(ClosureError::UnknownFact {
                            conclusion,
                            fact: fact.clone(),
                        });
                    };
                    fact_consumers.insert(conclusion.clone());
                }
                dependencies.insert(conclusion, required);
            }
        }
        if dependencies.is_empty() {
            return Err(ClosureError::EmptyClosure);
        }
        Ok(Self {
            owners,
            dependencies,
            consumers,
        })
    }

    /// Computes the exact conclusion closure affected by changed facts.
    #[must_use]
    pub fn affected(&self, changed: impl IntoIterator<Item = FactId>) -> Vec<ConclusionId> {
        changed
            .into_iter()
            .filter_map(|fact| self.consumers.get(&fact))
            .flatten()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Iterates over every declared conclusion in canonical order.
    pub fn conclusions(&self) -> impl ExactSizeIterator<Item = &ConclusionId> {
        self.dependencies.keys()
    }

    /// Returns whether one exact conclusion-to-fact dependency is declared.
    #[must_use]
    pub fn depends_on(&self, conclusion: &ConclusionId, fact: &FactId) -> bool {
        self.dependencies
            .get(conclusion)
            .is_some_and(|facts| facts.contains(fact))
    }

    /// Returns whether a fact belongs to the sealed semantic universe.
    #[must_use]
    pub fn contains_fact(&self, fact: &FactId) -> bool {
        self.consumers.contains_key(fact)
    }

    /// Explains one exact conclusion-to-fact dependency.
    pub fn explain(
        &self,
        conclusion: &ConclusionId,
        fact: &FactId,
    ) -> Result<Explanation, ClosureError> {
        let Some(required) = self.dependencies.get(conclusion) else {
            return Err(ClosureError::UnknownConclusion(conclusion.clone()));
        };
        if !required.contains(fact) {
            return Err(ClosureError::Unrelated {
                conclusion: conclusion.clone(),
                fact: fact.clone(),
            });
        }
        Ok(Explanation {
            conclusion: conclusion.clone(),
            fact: fact.clone(),
            path: vec![
                format!("owner/{}", self.owners[conclusion]),
                format!("conclusion/{conclusion}"),
                format!("fact/{fact}"),
            ],
        })
    }
}

/// Fail-closed federated-closure construction or explanation error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClosureError {
    /// Owner identity is empty.
    EmptyOwner,
    /// Owner graph has no conclusions.
    EmptyOwnerGraph(String),
    /// A conclusion has no semantic dependency.
    ConclusionWithoutFacts(ConclusionId),
    /// One owner declared a conclusion twice.
    DuplicateConclusion(ConclusionId),
    /// Two owners claimed one conclusion.
    DuplicateOwner {
        /// Conflicting conclusion.
        conclusion: ConclusionId,
        /// First owner.
        first: String,
        /// Second owner.
        second: String,
    },
    /// A graph referenced a fact outside the sealed world.
    UnknownFact {
        /// Referring conclusion.
        conclusion: ConclusionId,
        /// Missing fact.
        fact: FactId,
    },
    /// No owner graph contributed a conclusion.
    EmptyClosure,
    /// Requested conclusion is absent.
    UnknownConclusion(ConclusionId),
    /// Conclusion does not consume the requested fact.
    Unrelated {
        /// Requested conclusion.
        conclusion: ConclusionId,
        /// Requested fact.
        fact: FactId,
    },
}

impl fmt::Display for ClosureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl Error for ClosureError {}
