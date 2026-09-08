use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use sim_kernel::{ContentId, Datum, Symbol};

use super::{
    ConfinementEvidence, FederatedClosure, MediatedAccessWitness, ProjectionDigest,
    ProjectionError, ProjectionKindRef, ProjectionProvider, ProjectionResult, ProjectionSpec,
    ProjectorPolicy, ProjectorQualification, admission::content_id_datum, admission::policy_id,
};

/// Port used to validate a provider configuration through its declared Shape.
pub trait ConfigShapeVerifier {
    /// Rejects a config that does not match `shape`.
    fn verify(&self, shape: &ContentId, config: &Datum) -> Result<(), String>;
}

/// Open loaded registry keyed by projection kind.
#[derive(Default)]
pub struct ProjectionRegistry {
    providers: BTreeMap<ProjectionKindRef, RegisteredProvider>,
}

struct RegisteredProvider {
    identity: super::PackageIdentity,
    provider: Arc<dyn ProjectionProvider>,
}

impl ProjectionRegistry {
    /// Constructs an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Loads one provider without changing a central kind enum.
    pub fn register(
        &mut self,
        identity: super::PackageIdentity,
        provider: Arc<dyn ProjectionProvider>,
    ) -> Result<(), ProjectionError> {
        let kind = provider.kind().clone();
        if self
            .providers
            .insert(kind.clone(), RegisteredProvider { identity, provider })
            .is_some()
        {
            return Err(ProjectionError::DuplicateProvider(kind));
        }
        Ok(())
    }

    /// Resolves a loaded provider.
    #[must_use]
    pub fn get(
        &self,
        kind: &ProjectionKindRef,
    ) -> Option<(&super::PackageIdentity, &Arc<dyn ProjectionProvider>)> {
        self.providers
            .get(kind)
            .map(|registered| (&registered.identity, &registered.provider))
    }

    /// Lists loaded kinds in canonical order.
    pub fn kinds(&self) -> impl ExactSizeIterator<Item = &ProjectionKindRef> {
        self.providers.keys()
    }
}

/// Pure projection coordinator over a sealed world and federated closure.
pub struct ProjectionEngine<'a> {
    registry: &'a ProjectionRegistry,
    shapes: &'a dyn ConfigShapeVerifier,
    closure: &'a FederatedClosure,
}

impl<'a> ProjectionEngine<'a> {
    /// Binds the loaded registry, Shape verifier, and sealed closure.
    #[must_use]
    pub fn new(
        registry: &'a ProjectionRegistry,
        shapes: &'a dyn ConfigShapeVerifier,
        closure: &'a FederatedClosure,
    ) -> Self {
        Self {
            registry,
            shapes,
            closure,
        }
    }

    /// Runs one qualified projection without acquiring observation or effects.
    pub fn project(
        &self,
        world: &super::ObservedWorld,
        spec: &ProjectionSpec,
        policy: &ProjectorPolicy,
        qualification: Option<&ProjectorQualification>,
        confinement: Option<ConfinementEvidence>,
    ) -> Result<ProjectionResult, ProjectionError> {
        let qualification = qualification.ok_or_else(|| {
            ProjectionError::UnqualifiedProjector("projector qualification is missing".to_owned())
        })?;
        let (loaded_identity, provider) = self
            .registry
            .get(&spec.kind)
            .ok_or_else(|| ProjectionError::UnknownProvider(spec.kind.clone()))?;
        if loaded_identity != &spec.provider {
            return Err(ProjectionError::CodeIdentityMismatch);
        }
        if provider.config_shape() != &spec.config_shape {
            return Err(ProjectionError::ConfigShapeMismatch);
        }
        self.shapes
            .verify(&spec.config_shape, &spec.config)
            .map_err(ProjectionError::InvalidConfig)?;
        let expected_policy = policy_id(policy)
            .map_err(|error| ProjectionError::UnqualifiedProjector(error.to_string()))?;
        if qualification.policy() != &expected_policy {
            return Err(ProjectionError::UnqualifiedProjector(
                "qualification policy identity differs".to_owned(),
            ));
        }
        if qualification.implementation() != &spec.provider.code {
            return Err(ProjectionError::CodeIdentityMismatch);
        }
        if policy.requires_confinement {
            let evidence = confinement.as_ref().ok_or_else(|| {
                ProjectionError::UnavailableConfinement("required membrane absent".to_owned())
            })?;
            if !evidence.live {
                return Err(ProjectionError::UnavailableConfinement(
                    "required membrane unavailable on selected host".to_owned(),
                ));
            }
        }
        if policy.reads.facts().len() > policy.budgets.max_inputs {
            return Err(ProjectionError::BudgetExceeded("selected inputs"));
        }
        let inputs = world.select(&policy.reads)?;
        let output = provider.project(&inputs, &spec.config)?;
        let accessed = inputs.accessed();
        for fact in &accessed {
            if !output.dependencies.contains(fact) {
                return Err(ProjectionError::UndeclaredAccess {
                    accessed: fact.clone(),
                });
            }
        }
        for fact in &output.dependencies {
            if !accessed.contains(fact) {
                return Err(ProjectionError::UnreadDependency(fact.clone()));
            }
        }
        let output_bytes = output
            .value
            .canonical_bytes()
            .map_err(|error| ProjectionError::Canonical(error.to_string()))?;
        if output_bytes.len() > policy.budgets.max_output_bytes {
            return Err(ProjectionError::BudgetExceeded("projection output"));
        }
        let digest = projection_digest(
            spec,
            qualification,
            &inputs,
            &output.dependencies,
            &output.value,
        )?;
        let affected = self.closure.affected(output.dependencies.iter().cloned());
        let mut explanations = Vec::new();
        for conclusion in &affected {
            for fact in &output.dependencies {
                if let Ok(explanation) = self.closure.explain(conclusion, fact) {
                    explanations.push(explanation);
                }
            }
        }
        Ok(ProjectionResult {
            projection: output.value,
            mediated_access: MediatedAccessWitness {
                selected: policy.reads.facts().cloned().collect(),
                accessed,
            },
            projector_qualification: qualification.clone(),
            confinement,
            digest,
            affected,
            explanations,
        })
    }
}

fn projection_digest(
    spec: &ProjectionSpec,
    qualification: &ProjectorQualification,
    inputs: &super::ProjectionInputs,
    dependencies: &BTreeSet<super::FactId>,
    output: &Datum,
) -> Result<ProjectionDigest, ProjectionError> {
    let facts = dependencies
        .iter()
        .map(|id| {
            let value = inputs
                .get(id)
                .expect("provider dependency was validated as an accessed selected fact");
            Datum::Node {
                tag: Symbol::qualified("projection", "semantic-input-v1"),
                fields: vec![
                    (Symbol::new("id"), Datum::String(id.as_str().to_owned())),
                    (Symbol::new("value"), value.clone()),
                ],
            }
        })
        .collect();
    let preimage = Datum::Node {
        tag: Symbol::qualified("projection", "semantic-projection-digest-v1"),
        fields: vec![
            (
                Symbol::new("kind"),
                Datum::String(spec.kind.as_str().to_owned()),
            ),
            (
                Symbol::new("package"),
                Datum::String(spec.provider.name.clone()),
            ),
            (
                Symbol::new("version"),
                Datum::String(spec.provider.version.clone()),
            ),
            (
                Symbol::new("implementation"),
                content_id_datum(qualification.implementation()),
            ),
            (
                Symbol::new("policy"),
                content_id_datum(qualification.policy()),
            ),
            (Symbol::new("config"), spec.config.clone()),
            (Symbol::new("inputs"), Datum::Vector(facts)),
            (Symbol::new("projection"), output.clone()),
        ],
    };
    preimage
        .content_id()
        .map(ProjectionDigest)
        .map_err(|error| ProjectionError::Canonical(error.to_string()))
}
