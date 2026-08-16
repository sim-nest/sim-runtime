    fn direct_fixture(
        target_flags: u16,
        method_flags: u16,
    ) -> (
        ClassLoader,
        Arc<ClassDefinition>,
        JvmHeap,
        ManagedHandle,
        ManagedHandle,
    ) {
        let loader = ClassLoader::new(4096);
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let target = ClassDefinition::test(
            loader.id(),
            "target.Target",
            2,
            JavaClassMetadata::test_class(
                &cx,
                "target.Target",
                &[],
                target_flags,
                &[("run", "()V", method_flags)],
            ),
            BTreeMap::new(),
        );
        let owner = ClassDefinition::test(
            loader.id(),
            "caller.Owner",
            1,
            JavaClassMetadata::test_class(&cx, "caller.Owner", &[], 0x0001, &[]),
            BTreeMap::from([(
                7,
                SymbolicConstant::Member {
                    kind: ConstantResolutionKind::Method,
                    binary_name: "target.Target".into(),
                    name: "run".into(),
                    descriptor: "()V".into(),
                },
            )]),
        );
        loader.test_insert(target);
        loader.test_insert(owner.clone());
        let mut heap = JvmHeap::new(
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
        let cache = heap.allocate(JvmRole::Cache).unwrap();
        let owner_handle = heap.allocate(JvmRole::ClassMirror).unwrap();
        (loader, owner, heap, cache, owner_handle)
    }

    #[test]
    fn static_direct_handle_defers_initialization_until_invocation() {
        let (loader, owner, mut heap, cache, owner_handle) = direct_fixture(0x0001, 0x0009);
        let handle = resolve_direct_handle(
            &ResolutionCache::new(),
            &mut heap,
            cache,
            owner_handle,
            &loader,
            &owner,
            7,
            6,
            DirectReceiver::None,
        )
        .unwrap();
        assert_eq!(handle.kind(), DirectInvocationKind::Static);
        assert!(handle.initializes_on_invocation());
        assert_eq!(handle.declaring_class().id().loader(), loader.id());
    }

    #[test]
    fn interruption_stage_preserves_resume_and_performs_one_pipeline_effect_per_attempt() {
        struct Pipeline {
            calls: usize,
            resumes: Vec<Option<u8>>,
        }

        impl LambdaMethodPipeline for Pipeline {
            type Resume = u8;
            type Exception = sim_lib_control::Raised;

            fn invoke(
                &mut self,
                call: LambdaMethodCall<'_, Self::Resume>,
            ) -> Result<LambdaInvocationOutcome<Self::Resume, Self::Exception>, InvocationError>
            {
                self.calls += 1;
                self.resumes.push(call.resume);
                Ok(match call.resume {
                    None => LambdaInvocationOutcome::Interrupted {
                        resume: 41,
                        work: 7,
                    },
                    Some(41) => LambdaInvocationOutcome::Returned {
                        value: None,
                        work: 11,
                    },
                    Some(other) => panic!("linker changed resume evidence to {other}"),
                })
            }
        }

        let (loader, owner, mut heap, cache, owner_handle) = direct_fixture(0x0001, 0x0009);
        let implementation = resolve_direct_handle(
            &ResolutionCache::new(),
            &mut heap,
            cache,
            owner_handle,
            &loader,
            &owner,
            7,
            6,
            DirectReceiver::None,
        )
        .unwrap();
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let site = SiteKey {
            class: owner.id().clone(),
            method: MethodIdentity {
                name: "make".into(),
                descriptor: "()Lexample/Function;".into(),
            },
            constant_pool_index: 7,
            bootstrap: BootstrapMethod {
                method_handle: 3,
                arguments: Box::new([]),
            },
        };
        let functional = FunctionalInterface {
            interface: "example.Function".into(),
            method_name: "apply".into(),
            method_descriptor: "()V".into(),
            lineage: vec!["example.Function".into()],
        };
        let bootstrap = LambdaBootstrapPlan {
            sam_method_type: "()V".into(),
            implementation_reference_kind: 6,
            instantiated_method_type: "()V".into(),
            marker_interfaces: vec![],
            bridges: vec![],
            serializable: false,
        };
        let class = GeneratedLambdaClassSpace::new()
            .define(
                &cx,
                &mut heap,
                &loader,
                &owner,
                &site,
                "()Lexample/Function;",
                &functional,
                &bootstrap,
            )
            .unwrap();
        let plan = JvmFunctionPlan {
            neutral: neutral_plan(0, 0),
            body: JvmFunctionPolicyBody {
                adaptations: Box::new([]),
            },
        };
        let mut pipeline = Pipeline {
            calls: 0,
            resumes: vec![],
        };

        let interrupted = invoke_lambda_member(
            &mut pipeline,
            &class,
            &plan,
            &implementation,
            "apply",
            "()V",
            &[],
            vec![],
            None,
        )
        .unwrap();
        assert!(matches!(
            interrupted,
            LambdaInvocationOutcome::Interrupted {
                resume: 41,
                work: 7
            }
        ));
        let resumed = invoke_lambda_member(
            &mut pipeline,
            &class,
            &plan,
            &implementation,
            "apply",
            "()V",
            &[],
            vec![],
            Some(41),
        )
        .unwrap();
        assert!(matches!(
            resumed,
            LambdaInvocationOutcome::Returned {
                value: None,
                work: 11
            }
        ));
        assert_eq!(pipeline.calls, 2);
        assert_eq!(pipeline.resumes, [None, Some(41)]);
    }

    #[test]
    fn inaccessible_direct_target_fails_during_normative_resolution() {
        let (loader, owner, mut heap, cache, owner_handle) = direct_fixture(0, 0x0009);
        assert_eq!(
            resolve_direct_handle(
                &ResolutionCache::new(),
                &mut heap,
                cache,
                owner_handle,
                &loader,
                &owner,
                7,
                6,
                DirectReceiver::None,
            )
            .unwrap_err(),
            DirectHandleError::Resolution(ConstantResolutionError::IllegalAccess {
                binary_name: "target.Target".into(),
            })
        );
    }

    #[test]
    fn receiver_rules_and_unsupported_kinds_fail_closed() {
        let (loader, owner, mut heap, cache, owner_handle) = direct_fixture(0x0001, 0x0001);
        assert!(matches!(
            resolve_direct_handle(
                &ResolutionCache::new(),
                &mut heap,
                cache,
                owner_handle,
                &loader,
                &owner,
                7,
                5,
                DirectReceiver::None,
            ),
            Err(DirectHandleError::MissingReceiverRule)
        ));
        assert!(matches!(
            resolve_direct_handle(
                &ResolutionCache::new(),
                &mut heap,
                cache,
                owner_handle,
                &loader,
                &owner,
                7,
                1,
                DirectReceiver::None,
            ),
            Err(DirectHandleError::UnsupportedReferenceKind(1))
        ));
    }