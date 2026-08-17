#[test]
fn concurrent_requests_share_one_initialization() {
    let mut seed_cx = context();
    let root = Arc::new(MemoryDir::default());
    root.source(&mut seed_cx, "concurrent.sim", "\"once\"");
    let loader = Arc::new(ModuleLoader::new());
    let workers = (0..8)
        .map(|_| {
            let root = root.clone();
            let loader = loader.clone();
            std::thread::spawn(move || {
                let mut cx = context();
                loader
                    .load(&mut cx, request(root, "concurrent.sim", None))
                    .unwrap()
                    .generation()
            })
        })
        .collect::<Vec<_>>();
    assert!(
        workers
            .into_iter()
            .all(|worker| worker.join().unwrap() == 1)
    );
    let receipts = loader.receipts().unwrap();
    assert_eq!(
        receipts
            .iter()
            .filter(|r| r.outcome == ModuleResolutionOutcome::Linked)
            .count(),
        1
    );
    assert_eq!(
        receipts
            .iter()
            .filter(|r| r.outcome == ModuleResolutionOutcome::CacheHit)
            .count(),
        7
    );
}

#[derive(Debug)]
struct CompositionNode;

impl ManagedObject for CompositionNode {
    fn trace_edges(&self, _visitor: &mut dyn EdgeVisitor) {}

    fn clear_weak_edge(&mut self, _edge: EdgeId, _expected: ManagedId) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CompositionJob {
    Microtask,
    Finalization,
}

#[test]
fn language_neutral_organs_compose_through_one_source_module_lifecycle() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    root.source(&mut cx, "component.sim", "\"generation-one\"");

    let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(2).unwrap());
    let managed = arena.allocate(CompositionNode).unwrap();
    let rooted = arena.root(managed).unwrap();

    let parameter = Symbol::new("component");
    let argument = cx.factory().string("bound-component".to_owned()).unwrap();
    let bound = CallSignature::new()
        .with_positional(vec![CallParameter::required(parameter.clone())])
        .bind([CallArgument::Positional(argument.clone())])
        .unwrap();
    assert!(
        bound
            .get(&parameter)
            .is_some_and(|value| value == &argument)
    );

    let mut properties: PropertyStore<&str, &str, u64, ()> = PropertyStore::default();
    let generation = "generation";
    properties
        .define(
            &"component",
            generation,
            Descriptor::Data(DataDescriptor {
                value: 1,
                writable: true,
                enumerable: true,
                configurable: false,
            }),
        )
        .unwrap();
    assert!(matches!(
        properties.own(&"component", &generation),
        Some(Descriptor::Data(DataDescriptor { value, configurable: false, .. }))
            if *value == managed.id().allocation_ordinal() + 1
    ));

    let loader = ModuleLoader::new();
    let first = loader
        .load(&mut cx, request(root.clone(), "component.sim", None))
        .unwrap();
    let live_export = first.default_export().clone();
    assert_eq!(
        value_expr(&mut cx, &first),
        Expr::String("generation-one".to_owned())
    );

    let mut frame = ResumableFrame::new(
        FrameLimits { depth: 1, work: 2 },
        |packet: ResumePacket<u64, ()>, budget: &mut sim_lib_control::StepBudget| {
            budget.charge_work()?;
            match packet {
                ResumePacket::Start => Ok(ResumeResult::Yielded(1)),
                ResumePacket::Send(generation) => Ok(ResumeResult::Returned(generation)),
                ResumePacket::Throw(()) => Ok(ResumeResult::Failed(())),
                ResumePacket::Close => Ok(ResumeResult::Returned(0)),
            }
        },
    );
    assert_eq!(
        frame.resume(ResumePacket::Start),
        Ok(ResumeResult::Yielded(1))
    );

    let mut jobs = JobQueues::new(AdmissionLimit(3));
    jobs.enqueue(CompositionJob::Microtask, |jobs| {
        jobs.enqueue(CompositionJob::Microtask, |_| {}).unwrap();
    })
    .unwrap();
    jobs.enqueue(CompositionJob::Finalization, |_| {}).unwrap();
    let checkpoint = jobs
        .checkpoint(CompositionJob::Microtask, WorkLimit(2))
        .unwrap();
    assert_eq!(checkpoint.completed.len(), 2);
    assert!(
        jobs.drain(CompositionJob::Finalization, WorkLimit(0))
            .completed
            .is_empty()
    );

    root.source(&mut cx, "component.sim", "\"generation-two\"");
    let second = loader
        .reload(&mut cx, request(root, "component.sim", None))
        .unwrap();
    assert_eq!(
        frame.resume(ResumePacket::Send(second.generation())),
        Ok(ResumeResult::Returned(2))
    );
    assert_eq!(
        live_export
            .get()
            .unwrap()
            .object()
            .as_expr(&mut cx)
            .unwrap(),
        Expr::String("generation-two".to_owned())
    );

    let (_, safepoint) = arena
        .safepoint(|snapshot| snapshot.objects().count())
        .unwrap();
    assert_eq!(safepoint.roots, vec![managed.id()]);
    arena.release_root(rooted).unwrap();
    let teardown = arena.teardown();
    assert_eq!(teardown.objects, vec![managed.id()]);
    assert_eq!(
        loader
            .receipts()
            .unwrap()
            .iter()
            .map(|receipt| receipt.outcome)
            .collect::<Vec<_>>(),
        [
            ModuleResolutionOutcome::Linked,
            ModuleResolutionOutcome::Linked
        ]
    );
}
