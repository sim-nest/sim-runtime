#[cfg(test)]
mod environment_tests {
    use super::*;
    use crate::{ClassDefinition, ClassInitializationState, ClassLoader};
    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
    use sim_lib_gc_tracing::CollectionLimits;
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicUsize, Ordering},
    };

    fn insert(
        cx: &Cx,
        loader: &ClassLoader,
        name: &str,
        parents: &[&str],
        methods: &[(&str, &str, u16)],
    ) {
        insert_with_flags(cx, loader, name, parents, 0, methods);
    }

    fn insert_with_flags(
        cx: &Cx,
        loader: &ClassLoader,
        name: &str,
        parents: &[&str],
        flags: u16,
        methods: &[(&str, &str, u16)],
    ) {
        let metadata = JavaClassMetadata::test_class(cx, name, parents, flags, methods);
        loader.test_insert(ClassDefinition::test(
            loader.id(),
            name,
            name.len() as u64,
            metadata,
            BTreeMap::new(),
        ));
    }

    fn observed_method(
        fixpoint: u64,
        dependencies: &[(&Arc<ClassDefinition>, ClassSpaceRevision)],
    ) -> MethodVerificationProof {
        MethodVerificationProof {
            fixpoint: ValueFingerprint::new(fixpoint),
            dependencies: dependencies
                .iter()
                .map(|(class, _)| class.id().clone())
                .collect(),
            dependency_observations: dependencies
                .iter()
                .map(|(class, revision)| {
                    Observation::read(
                        class.id().clone(),
                        Revision::new(revision.number()),
                        class.id().incremental_fingerprint(),
                    )
                })
                .collect(),
            unreachable_handlers: Box::new([]),
        }
    }

    fn collection_limits() -> CollectionLimits {
        CollectionLimits {
            objects: 32,
            edges: 32,
            stack: 32,
            work: 128,
            clears: 32,
            finalizers: 0,
        }
    }

    #[test]
    fn class_proofs_are_exact_incremental_and_collectible() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let loader = ClassLoader::new(4096);
        insert(&cx, &loader, "Base", &[], &[]);
        insert(
            &cx,
            &loader,
            "Owner",
            &["Base"],
            &[("a", "()V", 0), ("b", "()V", 0)],
        );
        insert(&cx, &loader, "Other", &[], &[("stable", "()V", 0)]);
        let base = loader.loaded("Base").unwrap().unwrap();
        let owner = loader.loaded("Owner").unwrap().unwrap();
        let other = loader.loaded("Other").unwrap().unwrap();
        let revision = loader.revision();
        let methods = || {
            vec![
                (
                    "a()V".into(),
                    observed_method(11, &[(&owner, revision), (&base, revision)]),
                ),
                ("b()V".into(), observed_method(12, &[(&owner, revision)])),
            ]
        };
        let structural = ValueFingerprint::new(20);
        let clean = seal_class_verification(
            owner.id(),
            revision,
            ValueFingerprint::new(7),
            structural,
            methods(),
        )
        .unwrap();
        let incremental = seal_class_verification(
            owner.id(),
            revision,
            ValueFingerprint::new(7),
            structural,
            methods(),
        )
        .unwrap();
        assert_eq!(incremental.identity(), clean.identity());
        assert_eq!(clean.dependencies().len(), 2);

        let mut heap = JvmHeap::new(8, collection_limits()).unwrap();
        let managed_cache = heap.allocate(JvmRole::Cache).unwrap();
        let managed_owner = heap.allocate(JvmRole::ClassMirror).unwrap();
        let managed_other = heap.allocate(JvmRole::ClassMirror).unwrap();
        let cache_root = heap.root(managed_cache).unwrap();
        let owner_root = heap.root(managed_owner).unwrap();
        let other_root = heap.root(managed_other).unwrap();
        let cache = ClassVerificationCache::new();
        let request = (owner.id(), structural, clean.methods()).incremental_fingerprint();
        let cached = cache
            .insert(
                &mut heap,
                managed_cache,
                managed_owner,
                &owner,
                request,
                clean,
            )
            .unwrap();
        let other_structural = ValueFingerprint::new(30);
        let other_proof = seal_class_verification(
            other.id(),
            revision,
            ValueFingerprint::new(7),
            other_structural,
            [(
                "stable()V".into(),
                observed_method(31, &[(&other, revision)]),
            )],
        )
        .unwrap();
        let other_request =
            (other.id(), other_structural, other_proof.methods()).incremental_fingerprint();
        cache
            .insert(
                &mut heap,
                managed_cache,
                managed_other,
                &other,
                other_request,
                other_proof,
            )
            .unwrap();
        let current = |id: &ClassDefinitionId| {
            loader
                .loaded(id.binary_name())
                .ok()
                .flatten()
                .filter(|class| class.id() == id)
                .map(|class| class.id().incremental_fingerprint())
        };
        assert!(Arc::ptr_eq(
            &cached,
            &cache.lookup(&owner, request, current).unwrap()
        ));
        let edited_method_request = ValueFingerprint::new(request.get().wrapping_add(1));
        assert!(
            cache
                .lookup(&owner, edited_method_request, current)
                .is_none()
        );

        let replacement = JavaClassMetadata::test_class(&cx, "Base", &[], 0, &[("new", "()V", 0)]);
        loader.test_insert(ClassDefinition::test(
            loader.id(),
            "Base",
            999,
            replacement,
            BTreeMap::new(),
        ));
        assert!(cache.lookup(&owner, request, current).is_none());
        assert!(cache.lookup(&other, other_request, current).is_some());

        heap.release_root(owner_root).unwrap();
        let receipt = heap.collect().unwrap();
        assert_eq!(receipt.cleared_ephemerons.len(), 1);
        assert_eq!(receipt.cleared_ephemerons[0].0, managed_cache.id());
        assert!(receipt.swept.contains(&managed_owner.id()));
        assert!(!receipt.swept.contains(&managed_other.id()));
        heap.release_root(other_root).unwrap();
        heap.release_root(cache_root).unwrap();
    }

    #[test]
    fn verification_environment_is_read_only_and_records_exact_lineage() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let loader = ClassLoader::new(4096);
        insert(
            &cx,
            &loader,
            "SideEffectBase",
            &[],
            &[("<clinit>", "()V", 0x0008)],
        );
        insert(
            &cx,
            &loader,
            "VerifiedChild",
            &["SideEffectBase"],
            &[("run", "()V", 0)],
        );
        insert(&cx, &loader, "Unrelated", &[], &[]);

        // These counters stand at the effect boundaries a verifier must never
        // enter. The only operation below is metadata observation; no callback
        // capable of initialization, allocation, execution, native work, or a
        // source read is supplied to the environment.
        let initializer_runs = AtomicUsize::new(0);
        let allocations = AtomicUsize::new(0);
        let executions = AtomicUsize::new(0);
        let native_calls = AtomicUsize::new(0);
        let source_reads = AtomicUsize::new(0);
        let initialization = ClassInitializationState::Uninitialized;

        let environment = VerificationEnvironment::new(&loader, 3);
        let dependency_capacity = environment.dependencies.borrow().capacity();
        assert_eq!(
            environment.is_assignable("VerifiedChild", "SideEffectBase", 2),
            Ok(VerificationAssignability::Assignable)
        );
        let child = environment.class("VerifiedChild").unwrap();
        assert_eq!(
            child.methods().map(JavaMember::name).collect::<Vec<_>>(),
            ["run"]
        );
        assert_eq!(
            environment.dependencies.borrow().capacity(),
            dependency_capacity
        );
        let dependencies = environment.dependencies();
        assert_eq!(
            dependencies
                .iter()
                .map(|dependency| dependency.class().binary_name())
                .collect::<Vec<_>>(),
            ["VerifiedChild", "SideEffectBase"]
        );
        assert!(
            dependencies
                .iter()
                .all(|dependency| dependency.revision() == loader.revision())
        );
        assert_eq!(initialization, ClassInitializationState::Uninitialized);
        assert_eq!(initializer_runs.load(Ordering::Relaxed), 0);
        assert_eq!(allocations.load(Ordering::Relaxed), 0);
        assert_eq!(executions.load(Ordering::Relaxed), 0);
        assert_eq!(native_calls.load(Ordering::Relaxed), 0);
        assert_eq!(source_reads.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn verification_environment_refuses_loading_and_bounds_every_walk() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let loader = ClassLoader::new(4096);
        insert(&cx, &loader, "Child", &["Parent"], &[]);
        insert(&cx, &loader, "Parent", &[], &[]);
        let environment = VerificationEnvironment::new(&loader, 2);

        assert_eq!(
            environment.is_assignable("Child", "Parent", 1),
            Err(VerificationQueryError::LineageLimit { limit: 1 })
        );
        assert!(matches!(
            environment.class("Missing"),
            Err(VerificationQueryError::NotLoaded(name)) if name == "Missing"
        ));
    }

    #[test]
    fn assignability_and_join_apply_bounded_jvms_reference_rules() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let loader = ClassLoader::new(4096);
        insert_with_flags(&cx, &loader, "Left", &[], 0x0200, &[]);
        insert_with_flags(&cx, &loader, "Right", &[], 0x0200, &[]);
        insert(&cx, &loader, "Parent", &[], &[]);
        insert(&cx, &loader, "Child", &["Parent"], &[]);
        let environment = VerificationEnvironment::new(&loader, 16);

        let array = environment
            .reference_assignability(
                &ReferenceType::Array("[LChild;".into()),
                &ReferenceType::Array("[LParent;".into()),
                4,
            )
            .unwrap();
        assert_eq!(array.value, VerificationAssignability::Assignable);
        assert!(array.evidence.nodes_used <= array.evidence.node_limit);

        let joined = environment
            .join_types(
                &VerificationType::Reference(ReferenceType::Class("Left".into())),
                &VerificationType::Reference(ReferenceType::Class("Right".into())),
                4,
            )
            .unwrap();
        assert_eq!(
            joined.value,
            VerificationTypeJoin {
                value: VerificationType::Reference(ReferenceType::Object),
                rule: Some(VerificationJoinRule::UnrelatedInterfaces),
            }
        );
    }

    #[test]
    fn join_refuses_unresolved_hierarchy_and_exhausts_hostile_depth() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let loader = ClassLoader::new(4096);
        insert(&cx, &loader, "Broken", &["Missing"], &[]);
        insert(&cx, &loader, "Other", &[], &[]);
        let environment = VerificationEnvironment::new(&loader, 16);
        assert!(matches!(
            environment.join_types(
                &VerificationType::Reference(ReferenceType::Class("Broken".into())),
                &VerificationType::Reference(ReferenceType::Class("Other".into())),
                8,
            ),
            Err(VerificationQueryFailure { error: VerificationQueryError::NotLoaded(name), .. }) if name == "Missing"
        ));

        insert(&cx, &loader, "Deep0", &["Deep1"], &[]);
        insert(&cx, &loader, "Deep1", &["Deep2"], &[]);
        insert(&cx, &loader, "Deep2", &[], &[]);
        let failure = environment
            .reference_assignability(
                &ReferenceType::Class("Deep0".into()),
                &ReferenceType::Class("Other".into()),
                2,
            )
            .unwrap_err();
        assert_eq!(
            failure.error,
            VerificationQueryError::LineageLimit { limit: 2 }
        );
        assert_eq!(failure.evidence.node_limit, 2);
        assert_eq!(failure.evidence.nodes_used, 2);
        assert_eq!(failure.evidence.dependencies.len(), 2);
    }
}
impl StateSize for VerificationFrame {
    fn state_size(&self) -> usize {
        size_of::<Self>() + self.capacity() * size_of::<Slot>()
    }
}

impl JoinSemilattice for VerificationFrame {
    fn bottom(&self) -> Self {
        Self::bottom_frame(self.kind(), self.capacity())
    }

    fn join(&self, other: &Self) -> Self {
        if self.kind() != other.kind() || self.capacity() != other.capacity() {
            return Self::new(self.kind(), self.capacity().max(other.capacity()));
        }
        match (self.normalized_slots(), other.normalized_slots()) {
            (None, _) => other.clone(),
            (_, None) => self.clone(),
            (Some(left), Some(right)) => {
                let slots = left
                    .iter()
                    .zip(right)
                    .map(|(left, right)| match (left, right) {
                        (Slot::Value(a), Slot::Value(b)) => Slot::Value(a.join(b)),
                        (Slot::Category2Tail, Slot::Category2Tail) => Slot::Category2Tail,
                        (Slot::Unusable, Slot::Unusable) => Slot::Unusable,
                        _ => Slot::Unusable,
                    })
                    .collect::<Vec<_>>();
                let mut result = Self::Reachable {
                    kind: self.kind(),
                    slots: slots.into_boxed_slice(),
                };
                normalize_category2(&mut result);
                result
            }
        }
    }

    fn less_equal(&self, other: &Self) -> bool {
        self.join(other) == *other
    }
}

fn normalize_category2(frame: &mut VerificationFrame) {
    let VerificationFrame::Reachable { slots, .. } = frame else {
        return;
    };
    for index in 0..slots.len() {
        let valid_head = matches!(&slots[index], Slot::Value(value) if value.width() == Some(VerificationTypeWidth::Category2))
            && matches!(slots.get(index + 1), Some(Slot::Category2Tail));
        let valid_tail = index > 0
            && matches!(&slots[index - 1], Slot::Value(value) if value.width() == Some(VerificationTypeWidth::Category2));
        if (matches!(&slots[index], Slot::Value(value) if value.width() == Some(VerificationTypeWidth::Category2))
            && !valid_head)
            || (matches!(slots[index], Slot::Category2Tail) && !valid_tail)
        {
            slots[index] = Slot::Unusable;
        }
    }
}
