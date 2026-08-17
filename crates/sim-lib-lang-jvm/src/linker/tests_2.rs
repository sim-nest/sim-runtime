    fn fixture() -> (SiteKey, ClassLoader) {
        let loader = ClassLoader::new(4096);
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let class = crate::ClassDefinition::test(
            loader.id(),
            "Example",
            0x51_7e,
            JavaClassMetadata::test_identity(&cx, "Example", &[]),
            BTreeMap::new(),
        );
        (
            SiteKey {
                class: class.id().clone(),
                method: MethodIdentity {
                    name: "make".into(),
                    descriptor: "()V".into(),
                },
                constant_pool_index: 7,
                bootstrap: BootstrapMethod {
                    method_handle: 3,
                    arguments: vec![BootstrapArgument::Constant(11)].into_boxed_slice(),
                },
            },
            loader,
        )
}
    fn interface(
        cx: &Cx,
        name: &str,
        parents: &[&str],
        methods: &[(&str, &str, u16)],
    ) -> Arc<ClassDefinition> {
        ClassDefinition::test(
            ClassLoader::new(32).id(),
            name,
            1,
            JavaClassMetadata::test_class(cx, name, parents, 0x0601, methods),
            BTreeMap::new(),
        )
    }

    #[test]
    fn invalid_sam_discovery_stage_rejects_object_method_before_generation() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let classes = BTreeMap::from([(
            "example.EqualsOnly".into(),
            interface(
                &cx,
                "example.EqualsOnly",
                &[],
                &[("equals", "(Ljava/lang/Object;)Z", 0x0401)],
            ),
        )]);
        assert_eq!(
            discover_functional_interface(&classes, "example.EqualsOnly", 1),
            Err(FunctionalInterfaceError::NoAbstractMethod {
                interface: "example.EqualsOnly".into()
            })
        );
    }

    #[test]
    fn unrelated_abstract_methods_are_both_named() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let classes = BTreeMap::from([(
            "example.Pair".into(),
            interface(
                &cx,
                "example.Pair",
                &[],
                &[("left", "()V", 0x0401), ("right", "(I)V", 0x0401)],
            ),
        )]);
        assert_eq!(
            discover_functional_interface(&classes, "example.Pair", 1),
            Err(FunctionalInterfaceError::MultipleAbstractMethods {
                methods: vec!["left()V".into(), "right(I)V".into()]
            })
        );
    }

    #[test]
    fn recursive_sam_discovery_work_limit_stage_precedes_generation() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let classes = BTreeMap::from([
            (
                "example.Child".into(),
                interface(&cx, "example.Child", &["example.Parent"], &[]),
            ),
            (
                "example.Parent".into(),
                interface(&cx, "example.Parent", &[], &[("apply", "(I)I", 0x0401)]),
            ),
        ]);
        assert_eq!(
            discover_functional_interface(&classes, "example.Child", 1),
            Err(FunctionalInterfaceError::HierarchyBudgetExhausted { limit: 1 })
        );
        let found = discover_functional_interface(&classes, "example.Child", 2).unwrap();
        assert_eq!(
            (found.method_name.as_str(), found.method_descriptor.as_str()),
            ("apply", "(I)I")
        );
        assert_eq!(found.lineage, ["example.Child", "example.Parent"]);
    }

    #[test]
    fn inaccessible_handle_and_invalid_sam_stage_precede_generation() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let classes = BTreeMap::from([
            (
                "example.Function".into(),
                interface(
                    &cx,
                    "example.Function",
                    &[],
                    &[("apply", "(Ljava/lang/Object;)Ljava/lang/Object;", 0x0401)],
                ),
            ),
            (
                "example.Marker".into(),
                interface(&cx, "example.Marker", &[], &[]),
            ),
        ]);
        let plan = LambdaBootstrapPlan {
            sam_method_type: "(Ljava/lang/Object;)Ljava/lang/Object;".into(),
            implementation_reference_kind: 6,
            instantiated_method_type: "(Ljava/lang/String;)Ljava/lang/String;".into(),
            marker_interfaces: vec!["example.Marker".into()],
            bridges: vec!["(Ljava/lang/CharSequence;)Ljava/lang/Object;".into()],
            serializable: false,
        };
        let found = validate_functional_interface(
            &classes,
            "example.Capturing",
            "(I)Lexample/Function;",
            &plan,
            "(ILjava/lang/String;)Ljava/lang/String;",
            2,
        )
        .unwrap();
        assert_eq!(found.method_name, "apply");

        let incompatible = validate_functional_interface(
            &classes,
            "example.Capturing",
            "(I)Lexample/Function;",
            &plan,
            "(JLjava/lang/String;)Ljava/lang/String;",
            2,
        )
        .unwrap_err();
        assert!(matches!(
            incompatible,
            FunctionalInterfaceError::IncompatibleMethodType {
                role: "implementation",
                ..
            }
        ));

        let inaccessible = ClassDefinition::test(
            ClassLoader::new(32).id(),
            "other.HiddenMarker",
            1,
            JavaClassMetadata::test_class(&cx, "other.HiddenMarker", &[], 0x0600, &[]),
            BTreeMap::new(),
        );
        let mut inaccessible_classes = classes.clone();
        inaccessible_classes.insert("other.HiddenMarker".into(), inaccessible);
        let mut inaccessible_plan = plan.clone();
        inaccessible_plan.marker_interfaces = vec!["other.HiddenMarker".into()];
        assert_eq!(
            validate_functional_interface(
                &inaccessible_classes,
                "example.Capturing",
                "(I)Lexample/Function;",
                &inaccessible_plan,
                "(ILjava/lang/String;)Ljava/lang/String;",
                2,
            ),
            Err(FunctionalInterfaceError::InaccessibleInterface(
                "other.HiddenMarker".into()
            ))
        );
    }

    #[test]
    fn malformed_linkage_stage_performs_no_class_or_heap_effect() {
        let (_site, loader) = fixture();
        let classes = GeneratedLambdaClassSpace::new();
        let heap = JvmHeap::new(
            8,
            CollectionLimits {
                objects: 8,
                edges: 8,
                stack: 8,
                work: 32,
                clears: 8,
                finalizers: 0,
            },
        )
        .unwrap();
        let protocol = &executor_admitted_lambda_protocols()[1];
        let refused = decode_lambda_bootstrap(
            protocol.owner,
            protocol.name,
            protocol.descriptor,
            &[
                ResolvedBootstrapArgument::MethodType("()V".into()),
                ResolvedBootstrapArgument::MethodHandle { reference_kind: 6 },
                ResolvedBootstrapArgument::MethodType("()V".into()),
                ResolvedBootstrapArgument::Integer(8),
            ],
        );

        assert_eq!(
            refused,
            Err(LambdaBootstrapError::MalformedPayload(
                "unknown altMetafactory flag bit 3".into()
            ))
        );
        assert!(classes.browse(loader.id(), 1).is_empty());
        assert_eq!(heap.live_len(), 0);
    }

    #[test]
    fn identical_lambdas_at_two_occurrences_are_distinct_sites() {
        let (first, loader) = fixture();
        let mut second = first.clone();
        second.constant_pool_index = 8;
        assert_ne!(first, second);
        let mut cache = LinkageCache::new();
        let revision = loader.revision();
        let first_value = cache
            .resolve(first, revision, || Ok::<_, LinkageFailure>("first"))
            .unwrap();
        let second_value = cache
            .resolve(second, revision, || Ok::<_, LinkageFailure>("second"))
            .unwrap();
        assert_eq!((*first_value, *second_value), ("first", "second"));
    }

    #[test]
    fn stale_proof_stage_relinks_after_revision_change() {
        let (key, loader) = fixture();
        let revision = loader.revision();
        loader.simulate_class_space_change();
        let next = loader.revision();
        let mut successes = LinkageCache::new();
        let original = successes
            .resolve(key.clone(), revision, || Ok::<_, LinkageFailure>(1))
            .unwrap();
        let relinked = successes
            .resolve(key.clone(), next, || Ok::<_, LinkageFailure>(2))
            .unwrap();
        assert_eq!((*original, *relinked), (1, 2));

        let mut failures = LinkageCache::<u8>::new();
        let stale = LinkageFailure::Bootstrap("stale".into());
        assert_eq!(
            failures.resolve(key.clone(), revision, || Err(stale.clone())),
            Err(stale)
        );
        assert_eq!(*failures.resolve(key, next, || Ok(9)).unwrap(), 9);
    }

    #[test]
    fn cached_failure_stage_performs_no_later_link_effect() {
        let (key, loader) = fixture();
        let revision = loader.revision();
        let mut cache = LinkageCache::<u8>::new();
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let failure = LinkageFailure::InvalidConstantPoolEntry(7);

        for _ in 0..2 {
            assert_eq!(
                cache.resolve(key.clone(), revision, || {
                    attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Err(failure.clone())
                }),
                Err(failure.clone())
            );
        }
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(cache.state(&key, revision), LinkageState::Failed(failure));
    }

    #[test]
    fn allocation_limit_stage_precedes_graph_and_factory_effects() {
        let mut heap = JvmHeap::new(
            1,
            CollectionLimits {
                objects: 1,
                edges: 1,
                stack: 1,
                work: 1,
                clears: 1,
                finalizers: 0,
            },
        )
        .unwrap();
        heap.allocate(JvmRole::Object).unwrap();
        assert!(heap.allocate(JvmRole::Object).is_err());
        assert_eq!(heap.live_len(), 1);
    }
