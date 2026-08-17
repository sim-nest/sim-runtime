#[test]
fn neutral_source_policy_specimen_matches_the_frozen_capture() {
    use sim_lib_core::{DynamicSourcePolicy, ReadEvalOutcome, RequestOrigin};
    use sim_shape::{ExprKind, ExprKindShape};

    fn authority() -> SourceAuthority {
        SourceAuthority::new(
            ReadPolicy {
                trust: TrustLevel::TrustedSource,
                capabilities: CapabilitySet::new().grant(read_eval_capability()),
            },
            vec![module_load_capability()],
            CapabilitySet::new().grant(read_eval_capability()),
        )
        .unwrap()
    }

    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    root.source(&mut cx, "value.sim", "\"module-value\"");
    root.source(&mut cx, "broken.sim", "(");

    let modules = SourceModulePolicy::new(
        Symbol::qualified("codec", "lisp"),
        Arc::new(IdentitySpecifierPolicy),
    );
    let first = modules
        .load(
            &mut cx,
            Symbol::new("synthetic-root"),
            root.clone(),
            "value.sim",
            authority(),
        )
        .unwrap();
    let cached = modules
        .dynamic_import(
            &mut cx,
            Symbol::new("synthetic-root"),
            root.clone(),
            None,
            "value.sim",
            authority(),
        )
        .unwrap();
    assert_eq!(first.identity().path(), "value.sim");
    assert_eq!(cached.generation(), first.generation());

    let first_failure = modules
        .load(
            &mut cx,
            Symbol::new("synthetic-root"),
            root.clone(),
            "broken.sim",
            authority(),
        )
        .unwrap_err()
        .to_string();
    root.source(&mut cx, "broken.sim", "\"repair must remain cached\"");
    let cached_failure = modules
        .load(
            &mut cx,
            Symbol::new("synthetic-root"),
            root,
            "broken.sim",
            authority(),
        )
        .unwrap_err()
        .to_string();
    assert_eq!(cached_failure, first_failure);

    let dynamic = DynamicSourcePolicy::new(
        Symbol::qualified("codec", "lisp"),
        RequestOrigin::new(Symbol::qualified("specimen", "neutral-source")),
    );
    dynamic
        .evaluate_text(
            &mut cx,
            "\"dynamic-value\"",
            authority(),
            Arc::new(ExprKindShape::new(ExprKind::String)),
        )
        .unwrap();
    dynamic
        .evaluate_text(
            &mut cx,
            "nil",
            authority(),
            Arc::new(ExprKindShape::new(ExprKind::String)),
        )
        .unwrap_err();

    let cycle_identity = ModuleIdentity {
        root: Symbol::new("synthetic-root"),
        path: "cycle.sim".to_owned(),
    };
    modules.loader.state.lock().unwrap().cache.insert(
        cycle_identity,
        CacheState::Initializing {
            owner: std::thread::current().id(),
            generation: 1,
        },
    );
    modules
        .load(
            &mut cx,
            Symbol::new("synthetic-root"),
            Arc::new(MemoryDir::default()),
            "cycle.sim",
            authority(),
        )
        .unwrap_err();

    let receipts = modules.receipts().unwrap();
    let module_capture = receipts
        .iter()
        .map(|receipt| {
            (
                receipt.identity.path().to_owned(),
                receipt.generation,
                receipt.outcome,
                receipt.read_eval_event.as_ref().map(|event| event.seq),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        module_capture,
        vec![
            (
                "value.sim".to_owned(),
                1,
                ModuleResolutionOutcome::Linked,
                Some(0)
            ),
            (
                "value.sim".to_owned(),
                1,
                ModuleResolutionOutcome::CacheHit,
                None
            ),
            (
                "broken.sim".to_owned(),
                1,
                ModuleResolutionOutcome::DecodeFailed,
                Some(1)
            ),
            (
                "broken.sim".to_owned(),
                1,
                ModuleResolutionOutcome::DecodeFailed,
                None
            ),
            (
                "cycle.sim".to_owned(),
                1,
                ModuleResolutionOutcome::Cycle,
                None
            ),
        ]
    );

    let module_decisions = modules.decisions(&cx).unwrap();
    assert_eq!(module_decisions.len(), 2);
    assert!(module_decisions.iter().all(|decision| {
        decision.requested == vec![read_eval_capability()]
            && decision.active == vec![read_eval_capability()]
    }));
    assert_eq!(
        dynamic
            .decisions(&cx)
            .unwrap()
            .iter()
            .map(|decision| decision.outcome.clone())
            .collect::<Vec<_>>(),
        vec![ReadEvalOutcome::Admitted, ReadEvalOutcome::ShapeDenied]
    );
}
