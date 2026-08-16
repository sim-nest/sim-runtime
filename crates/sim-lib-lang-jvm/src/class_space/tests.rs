#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use sim_kernel::{
        CapabilitySet, ClassId, ClassRef, CodecId, DefaultFactory, EagerPolicy, Object,
        ObjectCompat, Origin, ReadPolicy, SourceId, Span, Table, TrustLevel, Value,
        read_eval_capability,
    };
    use sim_lib_machine::{
        AdmissionLimits, AdmissionPolicy, InstructionPolicy, LocatedCode, LocatedInstruction,
        MachineDescription, MachinePermit, SourceLocation,
    };

    use super::*;

    struct FixtureDir {
        bytes: Vec<u8>,
        reads: AtomicUsize,
    }

    impl FixtureDir {
        fn new() -> Self {
            Self {
                bytes: include_bytes!("../../fixtures/hand-built/Minimal.class").to_vec(),
                reads: AtomicUsize::new(0),
            }
        }
    }

    impl Object for FixtureDir {
        fn display(&self, _cx: &mut Cx) -> Result<String> {
            Ok("fixture class root".into())
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    impl ObjectCompat for FixtureDir {
        fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
            cx.factory()
                .class_stub(ClassId(0), Symbol::qualified("test", "ClassRoot"))
        }
        fn as_table_impl(&self) -> Option<&dyn Table> {
            Some(self)
        }
        fn as_dir(&self) -> Option<&dyn Dir> {
            Some(self)
        }
    }
    impl Table for FixtureDir {
        fn backend_symbol(&self) -> Symbol {
            Symbol::qualified("test", "class-root")
        }
        fn get(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            if key == Symbol::new("Minimal.class") {
                cx.factory().bytes(self.bytes.clone())
            } else {
                cx.factory().nil()
            }
        }
        fn set(&self, _cx: &mut Cx, _key: Symbol, _value: Value) -> Result<()> {
            Err(Error::Eval("read only".into()))
        }
        fn has(&self, _cx: &mut Cx, key: Symbol) -> Result<bool> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(key == Symbol::new("Minimal.class"))
        }
        fn del(&self, cx: &mut Cx, _key: Symbol) -> Result<Value> {
            cx.factory().nil()
        }
        fn keys(&self, _cx: &mut Cx) -> Result<Vec<Symbol>> {
            Ok(vec![Symbol::new("Minimal.class")])
        }
        fn entries(&self, _cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
            Ok(Vec::new())
        }
        fn len(&self, _cx: &mut Cx) -> Result<usize> {
            Ok(1)
        }
        fn clear(&self, _cx: &mut Cx) -> Result<()> {
            Err(Error::Eval("read only".into()))
        }
    }
    impl Dir for FixtureDir {
        fn mkdir(&self, _cx: &mut Cx, _name: Symbol) -> Result<Value> {
            Err(Error::Eval("read only".into()))
        }
        fn opendir(&self, _cx: &mut Cx, _name: Symbol) -> Result<Option<Value>> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(None)
        }
        fn rmdir(&self, cx: &mut Cx, _name: Symbol) -> Result<Value> {
            cx.factory().nil()
        }
        fn is_dir(&self, _cx: &mut Cx, _name: Symbol) -> Result<bool> {
            Ok(false)
        }
    }

    fn context(grant_load: bool) -> Cx {
        let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
        seat.grant(&mut cx, read_eval_capability()).unwrap();
        if grant_load {
            seat.grant(&mut cx, class_load_capability()).unwrap();
        }
        cx
    }

    fn authority() -> SourceAuthority {
        SourceAuthority::new(
            ReadPolicy {
                trust: TrustLevel::TrustedSource,
                capabilities: CapabilitySet::new().grant(read_eval_capability()),
            },
            vec![class_load_capability()],
            CapabilitySet::new()
                .grant(read_eval_capability())
                .grant(class_load_capability()),
        )
        .unwrap()
    }

    fn fixture_with_surrogate_literal() -> Vec<u8> {
        let mut bytes = include_bytes!("../../fixtures/hand-built/Minimal.class").to_vec();
        // The original pool ends at byte 0x42. Add #8 Utf8 containing one
        // supplementary character as its exact surrogate pair, and #9 String.
        bytes[8..10].copy_from_slice(&10_u16.to_be_bytes());
        bytes.splice(
            0x42..0x42,
            [1, 0, 6, 0xed, 0xa0, 0x80, 0xed, 0xb0, 0x80, 8, 0, 8],
        );
        bytes
    }

    #[test]
    fn loaded_code_unit_can_remain_a_lone_surrogate_through_jvm_operations() {
        let mut cx = context(true);
        let root = Arc::new(FixtureDir {
            bytes: fixture_with_surrogate_literal(),
            reads: AtomicUsize::new(0),
        });
        let loader = ClassLoader::with_intern_limit(4096, 2);
        let definition = loader
            .request(Symbol::new("classes"), root, "Minimal", authority())
            .unwrap()
            .resolve(&mut cx)
            .unwrap();
        let loaded = definition.string_literal(9).unwrap();
        assert_eq!(loaded.storage().as_code_units(), &[0xd800, 0xdc00]);
        let lone = loaded.substring(0, 1).unwrap();
        let interned = loader.intern(lone.storage()).unwrap();
        assert!(interned.content_equals(&lone));
        assert!(interned.identical(&loader.intern(lone.storage()).unwrap()));
        assert_eq!(
            interned.concat(&lone).unwrap().storage().as_code_units(),
            &[0xd800, 0xd800]
        );
        assert!(definition.mirror().identical(&definition.mirror()));
    }

    #[test]
    fn missing_power_is_refused_before_any_read_and_requests_are_lazy() {
        let mut cx = context(false);
        let root = Arc::new(FixtureDir::new());
        let loader = ClassLoader::new(4096);
        let request = loader
            .request(Symbol::new("classes"), root.clone(), "Minimal", authority())
            .unwrap();
        assert_eq!(
            root.reads.load(Ordering::SeqCst),
            0,
            "construction must not read or consult ambient state"
        );
        assert!(request.resolve(&mut cx).is_err());
        assert_eq!(
            root.reads.load(Ordering::SeqCst),
            0,
            "authority refusal must precede directory access"
        );
    }

    #[test]
    fn binary_names_cannot_escape_the_supplied_root() {
        let loader = ClassLoader::new(4096);
        let root = Arc::new(FixtureDir::new());
        assert!(
            loader
                .request(
                    Symbol::new("classes"),
                    root.clone(),
                    "../Minimal",
                    authority()
                )
                .is_err()
        );
        assert!(
            loader
                .request(Symbol::new("classes"), root, "/Minimal", authority())
                .is_err()
        );
    }

    #[test]
    fn loader_identity_partitions_content_bound_definitions() {
        let mut cx = context(true);
        let root = Arc::new(FixtureDir::new());
        let first_loader = ClassLoader::new(4096);
        let second_loader = ClassLoader::new(4096);
        let first = first_loader
            .request(Symbol::new("classes"), root.clone(), "Minimal", authority())
            .unwrap()
            .resolve(&mut cx)
            .unwrap();
        let second = second_loader
            .request(Symbol::new("classes"), root, "Minimal", authority())
            .unwrap()
            .resolve(&mut cx)
            .unwrap();
        assert_ne!(first.id(), second.id());
        assert_ne!(first.id().loader(), second.id().loader());
        assert_eq!(first.id().content_key(), second.id().content_key());
    }

    #[test]
    fn duplicate_binary_name_must_have_identical_content() {
        let mut cx = context(true);
        let root = Arc::new(FixtureDir::new());
        let loader = ClassLoader::new(4096);
        let first = loader
            .request(Symbol::new("classes"), root.clone(), "Minimal", authority())
            .unwrap()
            .resolve(&mut cx)
            .unwrap();
        let repeated = loader
            .request(Symbol::new("classes"), root, "Minimal", authority())
            .unwrap()
            .resolve(&mut cx)
            .unwrap();
        assert!(Arc::ptr_eq(&first, &repeated));
    }

    #[test]
    fn loaded_class_projects_a_browsable_shape_checked_class() {
        let mut cx = context(true);
        let definition = ClassLoader::new(4096)
            .request(
                Symbol::new("classes"),
                Arc::new(FixtureDir::new()),
                "Minimal",
                authority(),
            )
            .unwrap()
            .resolve(&mut cx)
            .unwrap();
        let class = definition.metadata().class_value(&cx, 16, 64).unwrap();
        assert!(class.object().as_class().is_some());
        let sample = cx
            .factory()
            .string("value supplied by Lisp".into())
            .unwrap();
        let checked = definition
            .metadata()
            .descriptor()
            .instance_shape()
            .object()
            .as_shape()
            .unwrap()
            .check_value(&mut cx, sample)
            .unwrap();
        assert!(checked.accepted);
        assert_eq!(
            definition.metadata().resolution().loader(),
            definition.id().loader()
        );
    }

    #[test]
    fn nested_array_class_identity_is_stable_and_component_derived() {
        let mut cx = context(true);
        let definition = ClassLoader::new(4096)
            .request(
                Symbol::new("classes"),
                Arc::new(FixtureDir::new()),
                "Minimal",
                authority(),
            )
            .unwrap()
            .resolve(&mut cx)
            .unwrap();
        let first = Arc::new(
            crate::JavaClassMetadata::array_of(&cx, definition.metadata().clone()).unwrap(),
        );
        let nested = crate::JavaClassMetadata::array_of(&cx, first.clone()).unwrap();
        let repeated = crate::JavaClassMetadata::array_of(&cx, first).unwrap();
        assert_eq!(
            nested.descriptor().identity().id(),
            repeated.descriptor().identity().id()
        );
        assert_eq!(nested.resolution().binary_name(), "[[Minimal");
        assert_eq!(
            nested.array_component().unwrap().resolution().binary_name(),
            "[Minimal"
        );
        assert_eq!(
            nested.is_assignable_to_binary_name("java.lang.Object", 1),
            crate::JavaHierarchyCheck::Match
        );
        assert_eq!(
            nested.is_assignable_to_binary_name("[[Minimal", 0),
            crate::JavaHierarchyCheck::BudgetExhausted { limit: 0 }
        );
    }

    #[test]
    fn java_method_selection_has_metadata_only_api_shape() {
        let _selector: for<'a> fn(
            &'a crate::JavaClassMetadata,
            &str,
            &str,
        ) -> Option<&'a crate::JavaMember> = crate::JavaClassMetadata::select_method;
    }

    struct TestInstructions;
    impl InstructionPolicy for TestInstructions {
        type Instruction = u8;
        type InstructionId = u8;
        fn instruction_id(instruction: &u8) -> u8 {
            *instruction
        }
    }
    struct TestAdmission;
    impl AdmissionPolicy<TestInstructions, ()> for TestAdmission {
        type Refusal = ();
        fn validate_description(
            _: &MachineDescription<'_, TestInstructions, ()>,
        ) -> std::result::Result<(), ()> {
            Ok(())
        }
        fn validate_instruction(_: &u8, _: &()) -> std::result::Result<(), ()> {
            Ok(())
        }
        fn encode_metadata(_: &(), _: &mut Vec<u8>) {}
        fn encode_instruction(instruction: &u8, output: &mut Vec<u8>) {
            output.push(*instruction);
        }
    }
    fn machine_permit() -> MachinePermit {
        let code = LocatedCode::<TestInstructions>::freeze(
            vec![LocatedInstruction::new(
                1,
                1,
                SourceLocation::Bytes(Origin {
                    codec: CodecId(1),
                    source: SourceId("jvm-entry-test".into()),
                    span: Span { start: 0, end: 1 },
                    trivia: vec![],
                }),
                false,
                None,
            )],
            vec![],
            vec![],
        )
        .unwrap();
        let description = MachineDescription::new(
            &code,
            AdmissionLimits {
                instructions: 1,
                operand_units: 1,
                slots: 1,
                frames: 1,
                work: 1,
            },
            &(),
        );
        MachinePermit::admit::<_, _, TestAdmission>(&description).unwrap()
    }

    #[test]
    fn one_entry_pipeline_defers_effects_and_rejects_stale_revision_by_identity() {
        let mut cx = context(true);
        let loader = ClassLoader::new(4096);
        let class = loader
            .request(
                Symbol::new("classes"),
                Arc::new(FixtureDir::new()),
                "Minimal",
                authority(),
            )
            .unwrap()
            .resolve(&mut cx)
            .unwrap();
        let allocations = AtomicUsize::new(0);
        let static_writes = AtomicUsize::new(0);
        let prepare = |target| {
            crate::ClassfilePermit::new(&loader, class.clone())
                .unwrap()
                .resolve(target)
                .unwrap()
                .admit(&machine_permit())
                .verify(&crate::NoVerifier)
                .unwrap()
                .permit()
        };
        let target = || ("value".into(), "()I".into());
        for target in [
            crate::EntryTarget::Method {
                name: target().0,
                descriptor: target().1,
            },
            crate::EntryTarget::Intrinsic {
                name: target().0,
                descriptor: target().1,
            },
            crate::EntryTarget::Dynamic {
                name: target().0,
                descriptor: target().1,
            },
        ] {
            crate::drive(prepare(target), || {}).unwrap();
        }
        let live = prepare(crate::EntryTarget::Method {
            name: target().0,
            descriptor: target().1,
        });
        assert_eq!(
            (
                allocations.load(Ordering::SeqCst),
                static_writes.load(Ordering::SeqCst)
            ),
            (0, 0)
        );
        assert_eq!(live.fidelity(), crate::VerificationFidelity::StaticChecked);
        crate::drive(live, || {
            allocations.fetch_add(1, Ordering::SeqCst);
            static_writes.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        assert_eq!(
            (
                allocations.load(Ordering::SeqCst),
                static_writes.load(Ordering::SeqCst)
            ),
            (1, 1)
        );

        let stale = prepare(crate::EntryTarget::Dynamic {
            name: target().0,
            descriptor: target().1,
        });
        let admitted = loader.revision();
        loader.simulate_class_space_change();
        let error = crate::drive(stale, || allocations.fetch_add(1, Ordering::SeqCst)).unwrap_err();
        assert!(
            matches!(error, crate::EntryRefusal::StaleClassSpace { admitted: found, current, .. }
            if found == admitted && current != admitted)
        );
        assert_eq!(
            allocations.load(Ordering::SeqCst),
            1,
            "stale refusal precedes the effect"
        );
    }

    #[test]
    fn verified_entry_requires_one_exact_complete_class_proof_before_effects() {
        use sim_incremental_core::ValueFingerprint;

        let mut cx = context(true);
        let loader = ClassLoader::new(4096);
        let class = loader
            .request(
                Symbol::new("classes"),
                Arc::new(FixtureDir::new()),
                "Minimal",
                authority(),
            )
            .unwrap()
            .resolve(&mut cx)
            .unwrap();
        let policy = ValueFingerprint::new(41);
        let structural = ValueFingerprint::new(42);
        let proof = Arc::new(crate::ClassVerificationProof::test(
            class.id().clone(),
            loader.revision(),
            policy,
            structural,
            &["value()I"],
        ));
        let wrong_method = Arc::new(crate::ClassVerificationProof::test(
            class.id().clone(),
            loader.revision(),
            policy,
            structural,
            &["other()I"],
        ));
        let foreign_loader = ClassLoader::new(4096);
        let foreign_class = foreign_loader
            .request(
                Symbol::new("classes"),
                Arc::new(FixtureDir::new()),
                "Minimal",
                authority(),
            )
            .unwrap()
            .resolve(&mut cx)
            .unwrap();
        let foreign = Arc::new(crate::ClassVerificationProof::test(
            foreign_class.id().clone(),
            foreign_loader.revision(),
            policy,
            structural,
            &["value()I"],
        ));
        let effects = AtomicUsize::new(0);
        let enter = |provider: &crate::ClassVerifierProvider| {
            crate::ClassfilePermit::new(&loader, class.clone())
                .unwrap()
                .resolve(crate::EntryTarget::Method {
                    name: "value".into(),
                    descriptor: "()I".into(),
                })
                .unwrap()
                .admit(&machine_permit())
                .verify(provider)
        };

        for provider in [
            crate::ClassVerifierProvider::exact(
                Arc::clone(&proof),
                ValueFingerprint::new(99),
                structural,
            ),
            crate::ClassVerifierProvider::failed(
                crate::VerificationProofFailure::Incomplete,
                policy,
                structural,
            ),
            crate::ClassVerifierProvider::failed(
                crate::VerificationProofFailure::BudgetExhausted,
                policy,
                structural,
            ),
            crate::ClassVerifierProvider::exact(wrong_method, policy, structural),
            crate::ClassVerifierProvider::exact(foreign, policy, structural),
        ] {
            assert!(enter(&provider).is_err());
        }
        assert_eq!(effects.load(Ordering::SeqCst), 0);

        let prepared = enter(&crate::ClassVerifierProvider::exact(
            proof, policy, structural,
        ))
        .unwrap();
        let permit = prepared.permit();
        assert_eq!(permit.fidelity(), crate::VerificationFidelity::Verified);
        crate::drive(permit, || effects.fetch_add(1, Ordering::SeqCst)).unwrap();
        assert_eq!(effects.load(Ordering::SeqCst), 1);
    }
}
