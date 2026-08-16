#[test]
fn failure_is_cached_and_receipted_deterministically() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    root.source(&mut cx, "bad.sim", "(");
    let loader = ModuleLoader::new();
    let first = loader
        .load(&mut cx, request(root.clone(), "bad.sim", None))
        .unwrap_err()
        .to_string();
    root.source(&mut cx, "bad.sim", "\"repaired but cached\"");
    let second = loader
        .load(&mut cx, request(root, "bad.sim", None))
        .unwrap_err()
        .to_string();
    assert_eq!(first, second);
    assert_eq!(
        loader
            .receipts()
            .unwrap()
            .iter()
            .map(|r| r.outcome)
            .collect::<Vec<_>>(),
        vec![
            ModuleResolutionOutcome::DecodeFailed,
            ModuleResolutionOutcome::DecodeFailed
        ]
    );
}
#[test]
fn lifecycle_receipts_link_exactly_one_decision_only_after_read() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    root.source(&mut cx, "good.sim", "\"linked\"");
    root.source(&mut cx, "decode.sim", "(");
    root.source(&mut cx, "eval.sim", "nil");
    let loader = ModuleLoader::new();

    loader
        .load(&mut cx, request(root.clone(), "missing.sim", None))
        .unwrap_err();
    loader
        .load(&mut cx, request(root.clone(), "decode.sim", None))
        .unwrap_err();
    let mut eval_request = request(root.clone(), "eval.sim", None);
    eval_request.authority = SourceAuthority::new(
        ReadPolicy {
            trust: TrustLevel::TrustedSource,
            capabilities: CapabilitySet::new().grant(read_eval_capability()),
        },
        vec![CapabilityName::new("test.missing")],
        CapabilitySet::new().grant(read_eval_capability()),
    )
    .unwrap();
    loader.load(&mut cx, eval_request).unwrap_err();
    loader
        .load(&mut cx, request(root, "good.sim", None))
        .unwrap();

    let receipts = loader.receipts().unwrap();
    assert_eq!(
        receipts
            .iter()
            .map(|receipt| receipt.outcome)
            .collect::<Vec<_>>(),
        vec![
            ModuleResolutionOutcome::ReadRefused,
            ModuleResolutionOutcome::DecodeFailed,
            ModuleResolutionOutcome::EvalFailed,
            ModuleResolutionOutcome::Linked,
        ]
    );
    assert!(receipts[0].read_eval_event.is_none());
    let linked = receipts[1..]
        .iter()
        .map(|receipt| receipt.read_eval_event.as_ref().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        linked.iter().map(|event| event.seq).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert_eq!(loader.decisions(&cx).unwrap().len(), linked.len());
}

#[test]
fn replacement_updates_existing_live_binding() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    root.source(&mut cx, "live.sim", "\"one\"");
    let loader = ModuleLoader::new();
    let first = loader
        .load(&mut cx, request(root.clone(), "live.sim", None))
        .unwrap();
    let edge = first.default_export().clone();
    root.source(&mut cx, "live.sim", "\"two\"");
    let second = loader
        .reload(&mut cx, request(root, "live.sim", None))
        .unwrap();
    assert_eq!(second.generation(), 2);
    assert_eq!(
        edge.get().unwrap().object().as_expr(&mut cx).unwrap(),
        Expr::String("two".to_owned())
    );
}

#[test]
fn initializing_cycle_has_stable_receipt_without_storage_access() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    let loader = ModuleLoader::new();
    let identity = ModuleIdentity {
        root: Symbol::new("fixture"),
        path: "cycle.sim".to_owned(),
    };
    loader.state.lock().unwrap().cache.insert(
        identity.clone(),
        CacheState::Initializing {
            owner: std::thread::current().id(),
            generation: 7,
        },
    );
    let error = loader
        .load(&mut cx, request(root, "cycle.sim", None))
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "evaluation error: module cycle at fixture:cycle.sim"
    );
    let receipt = loader.receipts().unwrap().pop().unwrap();
    assert_eq!(
        (receipt.generation, receipt.outcome),
        (7, ModuleResolutionOutcome::Cycle)
    );
}

#[test]
fn cache_hit_reuses_one_linked_generation() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    root.source(&mut cx, "shared.sim", "\"shared\"");
    let loader = ModuleLoader::new();
    let first = loader
        .load(&mut cx, request(root.clone(), "shared.sim", None))
        .unwrap();
    let second = loader
        .load(&mut cx, request(root, "shared.sim", None))
        .unwrap();
    assert_eq!(first.generation(), second.generation());
    assert_eq!(
        loader.receipts().unwrap().last().unwrap().outcome,
        ModuleResolutionOutcome::CacheHit
    );
}

#[test]
fn shared_policies_keep_codec_cache_and_authority_evidence_isolated() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    root.source(&mut cx, "shared.sim", "\"shared\"");
    let first = SourceModulePolicy::new(
        Symbol::qualified("codec", "lisp"),
        Arc::new(IdentitySpecifierPolicy),
    );
    let second = SourceModulePolicy::new(
        Symbol::qualified("codec", "missing"),
        Arc::new(IdentitySpecifierPolicy),
    );

    let loaded = first
        .load(
            &mut cx,
            Symbol::new("fixture"),
            root.clone(),
            "shared.sim",
            request(root.clone(), "unused.sim", None).authority,
        )
        .unwrap();
    first
        .dynamic_import(
            &mut cx,
            Symbol::new("fixture"),
            root.clone(),
            None,
            "shared.sim",
            request(root.clone(), "unused.sim", None).authority,
        )
        .unwrap();
    assert_eq!(loaded.generation(), 1);
    assert_eq!(first.receipts().unwrap().len(), 2);
    assert_eq!(first.decisions(&cx).unwrap().len(), 1);

    second
        .load(
            &mut cx,
            Symbol::new("fixture"),
            root.clone(),
            "shared.sim",
            request(root, "unused.sim", None).authority,
        )
        .unwrap_err();
    assert_eq!(second.receipts().unwrap().len(), 1);
    assert_eq!(second.decisions(&cx).unwrap().len(), 1);
    assert_eq!(first.receipts().unwrap().len(), 2);
    assert_eq!(first.decisions(&cx).unwrap().len(), 1);
}
