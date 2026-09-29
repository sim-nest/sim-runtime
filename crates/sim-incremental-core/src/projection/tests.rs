use std::{collections::BTreeSet, sync::Arc};

use sim_kernel::{ContentId, Datum};

use super::*;

fn datum_id(value: &str) -> ContentId {
    Datum::String(value.to_owned()).content_id().unwrap()
}

fn fact(value: &str, envelope: &str) -> ObservedFact {
    ObservedFact {
        semantic: Datum::String(value.to_owned()),
        envelope: Some(Datum::String(envelope.to_owned())),
    }
}

struct ExactShape(ContentId);

impl ConfigShapeVerifier for ExactShape {
    fn verify(&self, shape: &ContentId, config: &Datum) -> Result<(), String> {
        if shape != &self.0 {
            return Err("unknown Shape".to_owned());
        }
        if !matches!(config, Datum::String(value) if value == "checked") {
            return Err("config does not match projection/test-config-v1".to_owned());
        }
        Ok(())
    }
}

struct FirstFactProvider {
    kind: ProjectionKindRef,
    shape: ContentId,
    omit_dependency: bool,
}

impl ProjectionProvider for FirstFactProvider {
    fn kind(&self) -> &ProjectionKindRef {
        &self.kind
    }

    fn config_shape(&self) -> &ContentId {
        &self.shape
    }

    fn project(
        &self,
        inputs: &ProjectionInputs,
        _config: &Datum,
    ) -> Result<ProjectionOutput, ProjectionError> {
        let (id, value) = inputs
            .iter()
            .next()
            .ok_or(ProjectionError::BudgetExceeded("test input"))?;
        Ok(ProjectionOutput {
            value: value.clone(),
            dependencies: if self.omit_dependency {
                BTreeSet::new()
            } else {
                BTreeSet::from([id.clone()])
            },
        })
    }
}

fn policy(facts: &[&str], confinement: bool) -> ProjectorPolicy {
    ProjectorPolicy {
        input_shape: datum_id("input-shape"),
        reads: DeclaredInputSelector::new(facts.iter().map(|value| FactId::new(*value).unwrap())),
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
        requires_confinement: confinement,
    }
}

fn checker_owner() -> sim_conformance_core::OwnerBindingId {
    sim_conformance_core::OwnerBindingId::from_text("projection/test-owner").unwrap()
}

fn fixture(
    omit_dependency: bool,
    grant_admission: bool,
) -> (ProjectionRegistry, ContentId, ContentId, FederatedClosure) {
    let shape = datum_id("shape");
    let code = datum_id("provider-code");
    let kind = ProjectionKindRef::new("test/first-fact-v1").unwrap();
    let mut registry = ProjectionRegistry::new();
    registry
        .register(
            PackageIdentity {
                name: "test-provider".to_owned(),
                version: "1.0.0".to_owned(),
                code: code.clone(),
            },
            Arc::new(FirstFactProvider {
                kind: kind.clone(),
                shape: shape.clone(),
                omit_dependency,
            }),
        )
        .unwrap();
    if grant_admission {
        registry
            .admit_bootstrap_native(
                &kind,
                checker_owner(),
                code.clone(),
                datum_id("dependencies"),
            )
            .unwrap();
    }
    let closure = FederatedClosure::seal(
        [
            FactId::new("fact/a").unwrap(),
            FactId::new("fact/b").unwrap(),
        ],
        [
            OwnerProjectionGraph::new(
                "owner/a",
                [(
                    ConclusionId::new("conclusion/a").unwrap(),
                    BTreeSet::from([FactId::new("fact/a").unwrap()]),
                )],
            )
            .unwrap(),
            OwnerProjectionGraph::new(
                "owner/b",
                [(
                    ConclusionId::new("conclusion/b").unwrap(),
                    BTreeSet::from([FactId::new("fact/b").unwrap()]),
                )],
            )
            .unwrap(),
        ],
    )
    .unwrap();
    (registry, shape, code, closure)
}

fn spec(shape: ContentId, code: ContentId) -> ProjectionSpec {
    ProjectionSpec {
        id: datum_id("projection-request"),
        kind: ProjectionKindRef::new("test/first-fact-v1").unwrap(),
        config: Datum::String("checked".to_owned()),
        config_shape: shape,
        provider: PackageIdentity {
            name: "test-provider".to_owned(),
            version: "1.0.0".to_owned(),
            code,
        },
    }
}

#[test]
fn loaded_provider_needs_no_central_kind_enum_and_explains_exact_closure() {
    let (registry, shape, code, closure) = fixture(false, true);
    assert_eq!(
        registry
            .kinds()
            .map(ProjectionKindRef::as_str)
            .collect::<Vec<_>>(),
        ["test/first-fact-v1"]
    );
    let world = ObservedWorld::new([
        (FactId::new("fact/a").unwrap(), fact("alpha", "attempt 1")),
        (FactId::new("fact/b").unwrap(), fact("beta", "attempt 1")),
    ])
    .unwrap();
    let policy = policy(&["fact/a"], false);
    let result = ProjectionEngine::new(&registry, &ExactShape(shape.clone()), &closure)
        .project(&world, &spec(shape, code.clone()), &policy, None)
        .unwrap();
    assert_eq!(result.projection, Datum::String("alpha".to_owned()));
    assert_eq!(
        result.affected,
        [ConclusionId::new("conclusion/a").unwrap()]
    );
    assert_eq!(result.explanations.len(), 1);
    assert_eq!(
        result.explanations[0].path,
        ["owner/owner/a", "conclusion/conclusion/a", "fact/fact/a"]
    );
}

#[test]
fn envelopes_do_not_change_digest_but_semantic_input_does() {
    let (registry, shape, code, closure) = fixture(false, true);
    let project = |semantic: &str, envelope: &str| {
        let world =
            ObservedWorld::new([(FactId::new("fact/a").unwrap(), fact(semantic, envelope))])
                .unwrap();
        let policy = policy(&["fact/a"], false);
        ProjectionEngine::new(&registry, &ExactShape(shape.clone()), &closure)
            .project(&world, &spec(shape.clone(), code.clone()), &policy, None)
            .unwrap()
            .digest
    };
    assert_eq!(project("same", "host/a"), project("same", "host/b"));
    assert_ne!(project("same", "host/a"), project("changed", "host/a"));
}

#[test]
fn undeclared_access_and_shape_fail_for_distinct_reasons() {
    let (registry, shape, code, closure) = fixture(true, true);
    let world = ObservedWorld::new([(FactId::new("fact/a").unwrap(), fact("alpha", "diagnostic"))])
        .unwrap();
    let policy = policy(&["fact/a"], false);
    let shapes = ExactShape(shape.clone());
    let engine = ProjectionEngine::new(&registry, &shapes, &closure);
    assert!(matches!(
        engine.project(&world, &spec(shape.clone(), code.clone()), &policy, None),
        Err(ProjectionError::UndeclaredAccess { .. })
    ));
    let mut wrong = spec(shape, code);
    wrong.config_shape = datum_id("other-shape");
    assert_eq!(
        engine.project(&world, &wrong, &policy, None),
        Err(ProjectionError::ConfigShapeMismatch)
    );
}

#[test]
fn native_purity_requires_review_and_bwrap_is_separate() {
    let policy = policy(&["fact/a"], true);
    let declared = datum_id("native-proc-read-mutant");
    let loaded = datum_id("native-proc-read-mutant-different-build");
    let dependencies = datum_id("deps");
    assert_eq!(
        bootstrap_native_source(checker_owner(), declared, &loaded, &dependencies).unwrap_err(),
        QualificationError::NativeCodeMismatch
    );

    let (registry, shape, code, closure) = fixture(false, true);
    let world = ObservedWorld::new([(FactId::new("fact/a").unwrap(), fact("alpha", "diagnostic"))])
        .unwrap();
    let shapes = ExactShape(shape.clone());
    let engine = ProjectionEngine::new(&registry, &shapes, &closure);
    assert!(matches!(
        engine.project(&world, &spec(shape.clone(), code.clone()), &policy, None),
        Err(ProjectionError::UnavailableConfinement(_))
    ));
    assert!(matches!(
        engine.project(
            &world,
            &spec(shape, code),
            &policy,
            Some(ConfinementEvidence {
                membrane: "bwrap".to_owned(),
                policy: datum_id("bwrap-policy"),
                live: false,
            }),
        ),
        Err(ProjectionError::UnavailableConfinement(_))
    ));
}

#[test]
fn never_admitted_and_forged_loaded_identity_are_both_refused() {
    // Never granted bootstrap admission: no caller-suppliable qualification
    // exists to work around that anymore, so this is the only way to
    // reach `UnqualifiedProjector` now.
    let (registry, shape, code, closure) = fixture(false, false);
    let world =
        ObservedWorld::new([(FactId::new("fact/a").unwrap(), fact("alpha", "host/a"))]).unwrap();
    let policy = policy(&["fact/a"], false);
    let shapes = ExactShape(shape.clone());
    let engine = ProjectionEngine::new(&registry, &shapes, &closure);
    assert!(matches!(
        engine.project(&world, &spec(shape.clone(), code), &policy, None),
        Err(ProjectionError::UnqualifiedProjector(_))
    ));

    // Admitted this time, but the spec declares a code identity that
    // differs from what the registry actually has loaded.
    let (registry, shape, code, closure) = fixture(false, true);
    let shapes = ExactShape(shape.clone());
    let engine = ProjectionEngine::new(&registry, &shapes, &closure);
    let mut forged = spec(shape, code);
    forged.provider.code = datum_id("forged-code");
    assert_eq!(
        engine.project(&world, &forged, &policy, None),
        Err(ProjectionError::CodeIdentityMismatch)
    );
}

#[test]
fn wasm_clock_random_and_unqualified_semantics_are_refused() {
    let mut policy = policy(&["fact/a"], false);
    policy.imports.imports = BTreeSet::from(["wasi:clocks/wall-clock.now".to_owned()]);
    let runtime = QualifiedRuntime {
        code: datum_id("wasmi"),
        semantics: ExecutionSemantics {
            id: "wasmi/projection-v1".to_owned(),
            canonical_nan: true,
            canonical_collections: true,
            fresh_instance: true,
        },
    };
    let evidence = ClosedWasmEvidence {
        module: datum_id("module"),
        imports: policy.imports.clone(),
        runtime: runtime.clone(),
        admission: datum_id("admission"),
        import_manifest_complete: true,
        start_behavior_checked: true,
        budgets_enforced: true,
    };
    assert!(matches!(
        ProjectorQualificationVerifier::closed_wasm(&policy, evidence),
        Err(QualificationError::ForbiddenImport(_))
    ));

    policy.imports.imports.clear();
    let mut unqualified = runtime;
    unqualified.semantics.fresh_instance = false;
    assert_eq!(
        ProjectorQualificationVerifier::closed_wasm(
            &policy,
            ClosedWasmEvidence {
                module: datum_id("module"),
                imports: policy.imports.clone(),
                runtime: unqualified,
                admission: datum_id("admission"),
                import_manifest_complete: true,
                start_behavior_checked: true,
                budgets_enforced: true,
            },
        ),
        Err(QualificationError::RuntimeSemanticsUnqualified)
    );
}

#[test]
fn baseline_includes_disclosure_policy_and_is_open() {
    let mut registry = ProjectionRegistry::new();
    install_baseline_providers(
        &mut registry,
        datum_id("baseline-config-shape"),
        PackageIdentity {
            name: "sim-incremental-core".to_owned(),
            version: "0.5.0".to_owned(),
            code: datum_id("baseline-provider-code"),
        },
    )
    .unwrap();
    assert_eq!(registry.kinds().len(), BASELINE_PROJECTION_KINDS.len());
    assert!(
        registry
            .kinds()
            .any(|kind| kind.as_str() == "no-v3/disclosure-policy-v1")
    );
}

#[test]
fn path_glob_and_ignore_rules_are_explicit_and_canonical() {
    let rules = PathSelectionRules::new(
        ["src/**/*.rs".to_owned(), "Cargo.toml".to_owned()],
        ["src/generated/**".to_owned()],
    )
    .unwrap();
    assert_eq!(
        rules
            .select([
                "src/lib.rs".to_owned(),
                "src/nested/mod.rs".to_owned(),
                "src/generated/table.rs".to_owned(),
                "Cargo.toml".to_owned(),
                "README.md".to_owned(),
            ])
            .unwrap()
            .iter()
            .map(FactId::as_str)
            .collect::<Vec<_>>(),
        [
            "path/Cargo.toml",
            "path/src/lib.rs",
            "path/src/nested/mod.rs"
        ]
    );
    assert!(matches!(rules.config(), Datum::Node { .. }));
    assert!(matches!(
        rules.select(["../secret".to_owned()]),
        Err(ProjectionError::InvalidPathSelection(_))
    ));
    assert!(matches!(
        PathSelectionRules::new(Vec::new(), Vec::new()),
        Err(ProjectionError::InvalidPathSelection(_))
    ));
}

#[test]
fn federated_closure_refuses_overlap_and_unknown_facts() {
    let conclusion = ConclusionId::new("conclusion/shared").unwrap();
    let fact = FactId::new("fact/a").unwrap();
    let graph = |owner| {
        OwnerProjectionGraph::new(
            owner,
            [(conclusion.clone(), BTreeSet::from([fact.clone()]))],
        )
        .unwrap()
    };
    assert!(matches!(
        FederatedClosure::seal([fact.clone()], [graph("a"), graph("b")]),
        Err(ClosureError::DuplicateOwner { .. })
    ));
    assert!(matches!(
        FederatedClosure::seal([], [graph("a")]),
        Err(ClosureError::UnknownFact { .. })
    ));
}

#[test]
fn duplicate_registration_is_refused_and_the_original_provider_survives() {
    let (mut registry, shape, code, _closure) = fixture(false, true);
    let kind = ProjectionKindRef::new("test/first-fact-v1").unwrap();
    let attacker_code = datum_id("attacker-code");
    let result = registry.register(
        PackageIdentity {
            name: "attacker-provider".to_owned(),
            version: "1.0.0".to_owned(),
            code: attacker_code,
        },
        Arc::new(FirstFactProvider {
            kind: kind.clone(),
            shape,
            omit_dependency: false,
        }),
    );
    assert_eq!(
        result,
        Err(ProjectionError::DuplicateProvider(kind.clone()))
    );
    // The original provider and its real code identity must still be the
    // one loaded: registration failure must not have silently replaced it.
    let (loaded_identity, _provider) = registry.get(&kind).unwrap();
    assert_eq!(loaded_identity.code, code);
}
