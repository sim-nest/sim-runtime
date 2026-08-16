    use super::*;
    use crate::{ClassLoader, JavaClassMetadata, JvmRole, resolution::SymbolicConstant};
    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
    use sim_lib_binding::BindingCell;
    use sim_lib_function::{CallMode, CaptureDescriptor, ParameterDescriptor, ParameterKind};
    use sim_lib_gc_tracing::CollectionLimits;

    const LINKER_SOURCE: &str = concat!(
        include_str!("adaptation.rs"),
        include_str!("direct_handle.rs"),
        include_str!("bootstrap_model.rs"),
        include_str!("lambda_factory.rs"),
        include_str!("functional_interface.rs"),
        include_str!("linkage_cache.rs"),
    );

    fn neutral_plan(captures: usize, parameters: usize) -> FunctionPlan {
        FunctionPlan::new(
            sim_kernel::Symbol::new("jvm:test"),
            (0..parameters)
                .map(|index| {
                    ParameterDescriptor::new(
                        sim_kernel::Symbol::new(format!("p{index}")),
                        ParameterKind::Required,
                        CallMode::POSITIONAL,
                        None,
                    )
                })
                .collect(),
            (0..captures)
                .map(|index| {
                    CaptureDescriptor::new(sim_kernel::Symbol::new(format!("c{index}")), None)
                })
                .collect(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn java_lambda_is_an_ordinary_callable_and_sim_adapter_refuses_before_generation() {
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        cx.grant(crate::jvm_invoke_capability());
        let shape = cx.factory().opaque(Arc::new(AnyShape)).unwrap();
        let lambda = JavaLambdaCallable::new(vec![shape], None, |_cx, mut args| {
            JavaLambdaCallOutcome::Returned(args.remove(0))
        });
        let expected = cx.factory().string("shared function".into()).unwrap();
        let actual = lambda
            .call(&mut cx, Args::new(vec![expected.clone()]))
            .unwrap();
        assert_eq!(
            actual.object().display(&mut cx).unwrap(),
            expected.object().display(&mut cx).unwrap()
        );

        let value = cx.factory().opaque(Arc::new(lambda)).unwrap();
        let generated = std::sync::atomic::AtomicBool::new(false);
        let refused = adapt_sim_callable_as_functional_interface(&mut cx, value, || {
            generated.store(true, std::sync::atomic::Ordering::SeqCst);
            unreachable!("generation must follow capability admission")
        });
        assert!(matches!(
            refused,
            Err(FunctionalInterfaceError::InteropRefused(_))
        ));
        assert!(!generated.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn compiles_capture_receiver_parameter_and_return_policy_over_neutral_plan() {
        let compiled = compile_jvm_function_plan(
            neutral_plan(2, 2),
            "(Ljava/lang/String;I)Lexample/Fn;",
            "(Ljava/lang/Integer;I)Ljava/lang/Integer;",
            "(JILjava/lang/Object;)I",
            DirectReceiver::Bound,
            Some("Ljava/lang/Object;"),
        )
        .unwrap();
        assert_eq!(compiled.neutral().captures().len(), 2);
        assert_eq!(
            compiled.body().adaptations(),
            [
                LocatedJvmAdaptation {
                    point: AdaptationPoint::Receiver,
                    adaptation: JvmAdaptation::ReferenceCast {
                        from: "Ljava/lang/String;".into(),
                        to: "Ljava/lang/Object;".into(),
                    },
                },
                LocatedJvmAdaptation {
                    point: AdaptationPoint::Capture(1),
                    adaptation: JvmAdaptation::PrimitiveWiden { from: 'I', to: 'J' },
                },
                LocatedJvmAdaptation {
                    point: AdaptationPoint::Parameter(0),
                    adaptation: JvmAdaptation::Unbox {
                        reference: "Ljava/lang/Integer;".into(),
                        primitive: 'I',
                    },
                },
                LocatedJvmAdaptation {
                    point: AdaptationPoint::Parameter(1),
                    adaptation: JvmAdaptation::Box {
                        primitive: 'I',
                        reference: "Ljava/lang/Object;".into(),
                    },
                },
                LocatedJvmAdaptation {
                    point: AdaptationPoint::Return,
                    adaptation: JvmAdaptation::Box {
                        primitive: 'I',
                        reference: "Ljava/lang/Integer;".into(),
                    },
                },
            ]
        );
    }

    #[test]
    fn bad_adaptation_stage_precedes_factory_allocation_and_invocation() {
        assert_eq!(
            compile_jvm_function_plan(
                neutral_plan(0, 1),
                "()Lexample/Fn;",
                "(J)I",
                "(I)I",
                DirectReceiver::None,
                None,
            ),
            Err(JvmAdaptationError::UnsupportedConversion {
                point: AdaptationPoint::Parameter(0),
                from: "J".into(),
                to: "I".into(),
            })
        );
        assert_eq!(
            compile_jvm_function_plan(
                neutral_plan(0, 0),
                "()Lexample/Fn;",
                "()I",
                "()V",
                DirectReceiver::None,
                None,
            ),
            Err(JvmAdaptationError::VoidToValue {
                point: AdaptationPoint::Return,
                required: "I".into(),
            })
        );
    }

    #[test]
    fn neutral_function_sources_contain_no_java_descriptor_vocabulary() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../sim-lib-function/src");
        let forbidden = [
            "java/lang",
            "LambdaMetafactory",
            "MethodType",
            "invokedynamic",
        ];
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|extension| extension == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                for vocabulary in forbidden {
                    assert!(
                        !source.contains(vocabulary),
                        "neutral source {} contains JVM vocabulary {vocabulary}",
                        path.display()
                    );
                }
            }
        }
    }

    #[test]
    fn generated_lambda_class_is_stable_browsable_and_shape_checked_without_bytes() {
        let (site, loader) = fixture();
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let mut heap = JvmHeap::new(
            32,
            CollectionLimits {
                objects: 32,
                edges: 32,
                stack: 32,
                work: 128,
                clears: 32,
                finalizers: 0,
            },
        )
        .unwrap();
        let functional = FunctionalInterface {
            interface: "example.Function".into(),
            method_name: "apply".into(),
            method_descriptor: "(Ljava/lang/Object;)Ljava/lang/Object;".into(),
            lineage: vec!["example.Function".into()],
        };
        let plan = LambdaBootstrapPlan {
            sam_method_type: functional.method_descriptor.clone(),
            implementation_reference_kind: 6,
            instantiated_method_type: "(Ljava/lang/String;)Ljava/lang/String;".into(),
            marker_interfaces: vec!["example.Marker".into()],
            bridges: vec!["(Ljava/lang/CharSequence;)Ljava/lang/Object;".into()],
            serializable: true,
        };
        let owner = ClassDefinition::test(
            loader.id(),
            site.class.binary_name(),
            site.class.content_key(),
            JavaClassMetadata::test_identity(&cx, site.class.binary_name(), &[]),
            BTreeMap::new(),
        );
        let mut classes = GeneratedLambdaClassSpace::new();
        let first = classes
            .define(
                &cx,
                &mut heap,
                &loader,
                &owner,
                &site,
                "(I)Lexample/Function;",
                &functional,
                &plan,
            )
            .unwrap();
        let repeated = classes
            .define(
                &cx,
                &mut heap,
                &loader,
                &owner,
                &site,
                "(I)Lexample/Function;",
                &functional,
                &plan,
            )
            .unwrap();

        assert!(Arc::ptr_eq(&first, &repeated));
        assert_eq!(classes.browse(loader.id(), 8).len(), 1);
        assert_eq!(first.members().len(), 3);
        assert_eq!(first.members()[2].role(), GeneratedLambdaMemberRole::Bridge);
        let sam = first
            .select_invocation_member("apply", "(Ljava/lang/String;)Ljava/lang/String;")
            .unwrap();
        let bridge = first
            .select_invocation_member("apply", "(Ljava/lang/CharSequence;)Ljava/lang/Object;")
            .unwrap();
        assert_eq!(sam.role(), GeneratedLambdaMemberRole::Sam);
        assert_eq!(bridge.role(), GeneratedLambdaMemberRole::Bridge);
        assert!(
            first
                .select_invocation_member("apply", "(Ljava/lang/Object;)Ljava/lang/Object;")
                .is_none(),
            "selection must not fall back across erasures"
        );
        assert_eq!(
            first
                .descriptor()
                .parents()
                .iter()
                .map(|parent| parent.identity().symbol().name.as_ref())
                .collect::<Vec<_>>(),
            ["example.Function", "example.Marker", "java.io.Serializable"]
        );
        assert!(
            first
                .class_value(&cx, 16, 64)
                .unwrap()
                .object()
                .as_class()
                .is_some()
        );
        let sample = cx.factory().string("lambda instance".into()).unwrap();
        let checked = first
            .descriptor()
            .instance_shape()
            .object()
            .as_shape()
            .unwrap()
            .check_value(&mut cx, sample)
            .unwrap();
        assert!(checked.accepted);

        let source = LINKER_SOURCE;
        assert!(!source.contains(concat!("0xCAFE", "BABE")));
        assert!(!source.contains(concat!("define_", "bytes(")));
        assert!(!source.contains(concat!("Class", "Shell")));
    }

    #[test]
    fn loader_collection_stage_collects_a_captured_lambda_enclosing_object_cycle() {
        let (site, loader) = fixture();
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let owner = ClassDefinition::test(
            loader.id(),
            site.class.binary_name(),
            site.class.content_key(),
            JavaClassMetadata::test_identity(&cx, site.class.binary_name(), &[]),
            BTreeMap::new(),
        );
        let mut heap = JvmHeap::new(
            32,
            CollectionLimits {
                objects: 32,
                edges: 64,
                stack: 32,
                work: 256,
                clears: 32,
                finalizers: 0,
            },
        )
        .unwrap();
        let loader_node = heap.allocate(JvmRole::Loader).unwrap();
        let loader_root = heap.root(loader_node).unwrap();
        let owner_node = heap.allocate(JvmRole::ClassMirror).unwrap();
        heap.strong(loader_node, crate::JvmEdge::DefinedClass, owner_node)
            .unwrap();
        heap.strong(owner_node, crate::JvmEdge::DefiningLoader, loader_node)
            .unwrap();
        let cache_node = heap.allocate(JvmRole::Cache).unwrap();
        let _cache_root = heap.root(cache_node).unwrap();
        let capture_node = heap.allocate(JvmRole::Object).unwrap();
        let capture_root = heap.root(capture_node).unwrap();

        let functional = FunctionalInterface {
            interface: "example.Function".into(),
            method_name: "apply".into(),
            method_descriptor: "()Ljava/lang/Object;".into(),
            lineage: vec!["example.Function".into()],
        };
        let bootstrap = LambdaBootstrapPlan {
            sam_method_type: functional.method_descriptor.clone(),
            implementation_reference_kind: 6,
            instantiated_method_type: functional.method_descriptor.clone(),
            marker_interfaces: vec![],
            bridges: vec![],
            serializable: true,
        };
        let mut classes = GeneratedLambdaClassSpace::new();
        let generated = classes
            .define(
                &cx,
                &mut heap,
                &loader,
                &owner,
                &site,
                "(Ljava/lang/Object;)Lexample/Function;",
                &functional,
                &bootstrap,
            )
            .unwrap();
        let class_value = generated.class_value(&cx, 16, 64).unwrap();
        let plan = JvmFunctionPlan {
            neutral: neutral_plan(1, 0),
            body: JvmFunctionPolicyBody {
                adaptations: Box::new([]),
            },
        };
        let mut factories = LambdaFactoryCache::default();
        assert!(matches!(
            factories.link(
                &mut heap,
                cache_node,
                owner_node,
                &owner,
                site.clone(),
                generated.clone(),
                plan.clone(),
                class_value.clone(),
                StatelessLambdaIdentity::PermittedSingleton,
            ),
            Err(LambdaFactoryError::CapturingSingleton)
        ));
        let first_factory = factories
            .link(
                &mut heap,
                cache_node,
                owner_node,
                &owner,
                site.clone(),
                generated.clone(),
                plan.clone(),
                class_value.clone(),
                StatelessLambdaIdentity::Fresh,
            )
            .unwrap();
        let repeated_factory = factories
            .link(
                &mut heap,
                cache_node,
                owner_node,
                &owner,
                site.clone(),
                generated.clone(),
                plan,
                class_value,
                StatelessLambdaIdentity::Fresh,
            )
            .unwrap()
            .clone();
        assert!(Arc::ptr_eq(&first_factory, &repeated_factory));
        let factory_node = first_factory.lock().unwrap().managed();

        let captured_value = cx.factory().string("captured".into()).unwrap();
        let binding = || {
            CapturedBinding::new(
                BindingCell::initialized(Symbol::new("c0"), captured_value.clone()),
                capture_node,
            )
        };
        let first_instance = first_factory
            .lock()
            .unwrap()
            .instantiate(&mut heap, vec![binding()])
            .unwrap();
        heap.strong(
            capture_node,
            crate::JvmEdge::Field,
            first_instance.managed(),
        )
        .unwrap();
        let second_instance = first_factory
            .lock()
            .unwrap()
            .instantiate(&mut heap, vec![binding()])
            .unwrap();
        assert_ne!(first_instance.managed(), second_instance.managed());
        assert_eq!(
            first_instance.function().captures()[0]
                .cell()
                .get()
                .unwrap(),
            captured_value
        );
        assert_eq!(
            first_instance.serialized_replacement(&generated),
            Err(LambdaSerializationError::ManagedReplacementUnavailable {
                loader: loader.id(),
                class: generated.binary_name().to_owned(),
                object: first_instance.managed(),
            })
        );
        let source = LINKER_SOURCE;
        assert!(!source.contains(concat!("Object", "OutputStream")));
        assert!(!source.contains(concat!("bincode", "::serialize")));
        let first_instance_node = first_instance.managed();
        first_instance.release(&mut heap).unwrap();
        second_instance.release(&mut heap).unwrap();

        let mut stateless_site = site.clone();
        stateless_site.constant_pool_index += 1;
        let stateless = factories
            .link(
                &mut heap,
                cache_node,
                owner_node,
                &owner,
                stateless_site,
                generated.clone(),
                JvmFunctionPlan {
                    neutral: neutral_plan(0, 0),
                    body: JvmFunctionPolicyBody {
                        adaptations: Box::new([]),
                    },
                },
                generated.class_value(&cx, 16, 64).unwrap(),
                StatelessLambdaIdentity::PermittedSingleton,
            )
            .unwrap();
        let stateless_first = stateless
            .lock()
            .unwrap()
            .instantiate(&mut heap, vec![])
            .unwrap();
        let stateless_second = stateless
            .lock()
            .unwrap()
            .instantiate(&mut heap, vec![])
            .unwrap();
        assert_eq!(stateless_first.managed(), stateless_second.managed());
        stateless_first.release(&mut heap).unwrap();
        stateless_second.release(&mut heap).unwrap();
        heap.release_root(capture_root).unwrap();

        let weak_factory = Arc::downgrade(&first_factory);
        let weak_class = Arc::downgrade(&generated);
        drop(repeated_factory);
        drop(first_factory);
        drop(stateless);
        drop(generated);
        drop(owner);
        assert_eq!(factories.live_len(), 0);
        assert_eq!(classes.live_len(), 0);
        assert!(weak_factory.upgrade().is_none());
        assert!(weak_class.upgrade().is_none());
        heap.release_root(loader_root).unwrap();
        let receipt = heap.collect().unwrap();
        assert!(receipt.swept.contains(&factory_node.id()));
        assert!(receipt.swept.contains(&capture_node.id()));
        assert!(receipt.swept.contains(&first_instance_node.id()));
        assert_eq!(receipt.cleared_ephemerons.len(), 2);
        assert!(
            receipt
                .cleared_ephemerons
                .iter()
                .all(|(owner, _)| *owner == cache_node.id())
        );
    }
