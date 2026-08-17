/// One method's stable identity inside a whole-class verification proof.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClassMethodProofIdentity {
    method: String,
    proof: ValueFingerprint,
}
impl ClassMethodProofIdentity {
    /// Binds a declared method identity to its completed dataflow proof.
    pub fn new(method: impl Into<String>, proof: ValueFingerprint) -> Self {
        Self {
            method: method.into(),
            proof,
        }
    }

    /// Stable declared method identity (normally name plus descriptor).
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Stable whole-method proof identity.
    pub const fn proof(&self) -> ValueFingerprint {
        self.proof
    }
}

/// Immutable proof for every structural constraint and method of one exact class.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassVerificationProof {
    owner: ClassDefinitionId,
    owner_revision: ClassSpaceRevision,
    policy: ValueFingerprint,
    structural: ValueFingerprint,
    methods: Box<[ClassMethodProofIdentity]>,
    dependencies: Box<[Observation<ClassDefinitionId>]>,
    identity: ValueFingerprint,
}

impl ClassVerificationProof {
    #[cfg(test)]
    pub(crate) fn test(
        owner: ClassDefinitionId,
        owner_revision: ClassSpaceRevision,
        policy: ValueFingerprint,
        structural: ValueFingerprint,
        methods: &[&str],
    ) -> Self {
        let methods = methods
            .iter()
            .enumerate()
            .map(|(index, method)| {
                ClassMethodProofIdentity::new(*method, ValueFingerprint::new(index as u64 + 1))
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            owner,
            owner_revision,
            policy,
            structural,
            methods,
            dependencies: Box::new([]),
            identity: ValueFingerprint::new(99),
        }
    }

    /// Exact class definition proved.
    pub fn owner(&self) -> &ClassDefinitionId {
        &self.owner
    }

    /// Exact class-space revision observed while sealing this proof.
    pub const fn owner_revision(&self) -> ClassSpaceRevision {
        self.owner_revision
    }

    /// Exact verifier policy and schema used to produce this proof.
    pub const fn policy_fingerprint(&self) -> ValueFingerprint {
        self.policy
    }

    /// Fingerprint of class-level constraints (header, members, and attributes).
    pub const fn structural_fingerprint(&self) -> ValueFingerprint {
        self.structural
    }

    /// Method proofs in stable declared-method order.
    pub fn methods(&self) -> &[ClassMethodProofIdentity] {
        &self.methods
    }

    /// Deduplicated exact dependency observations, ordered by class identity.
    pub fn dependencies(&self) -> &[Observation<ClassDefinitionId>] {
        &self.dependencies
    }

    /// Content identity equal for incremental and clean recomputation.
    pub const fn identity(&self) -> ValueFingerprint {
        self.identity
    }
}

/// Refusal to aggregate incomplete or ambiguous method evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClassVerificationError {
    /// Two proofs claimed the same declared method identity.
    DuplicateMethod(String),
}

/// Aggregates stable method proofs and structural evidence into one exact class proof.
pub fn seal_class_verification(
    owner: &ClassDefinitionId,
    owner_revision: ClassSpaceRevision,
    policy: ValueFingerprint,
    structural: ValueFingerprint,
    methods: impl IntoIterator<Item = (String, MethodVerificationProof)>,
) -> Result<ClassVerificationProof, ClassVerificationError> {
    let mut identities = Vec::new();
    let mut dependencies = BTreeMap::new();
    dependencies.insert(
        owner.clone(),
        Observation::read(
            owner.clone(),
            Revision::new(owner_revision.number()),
            owner.incremental_fingerprint(),
        ),
    );
    for (method, proof) in methods {
        if identities
            .iter()
            .any(|identity: &ClassMethodProofIdentity| identity.method == method)
        {
            return Err(ClassVerificationError::DuplicateMethod(method));
        }
        identities.push(ClassMethodProofIdentity::new(method, proof.fixpoint));
        for observation in proof.dependency_observations {
            dependencies.insert(observation.key().clone(), observation);
        }
    }
    identities.sort();
    let dependencies = dependencies.into_values().collect::<Vec<_>>();
    let identity = (
        owner,
        policy,
        structural,
        &identities,
        dependencies
            .iter()
            .map(|observation| (observation.key(), observation.fingerprint()))
            .collect::<Vec<_>>(),
    )
        .incremental_fingerprint();
    Ok(ClassVerificationProof {
        owner: owner.clone(),
        owner_revision,
        policy,
        structural,
        methods: identities.into_boxed_slice(),
        dependencies: dependencies.into_boxed_slice(),
        identity,
    })
}

struct ClassProofCacheEntry {
    owner: Weak<ClassDefinition>,
    request: ValueFingerprint,
    proof: Arc<ClassVerificationProof>,
    _managed_proof: sim_lib_mutation::ManagedHandle,
}

/// Whole-class proof memo whose managed entries are ephemerons keyed by class mirrors.
#[derive(Default)]
pub struct ClassVerificationCache {
    entries: Mutex<BTreeMap<ClassDefinitionId, ClassProofCacheEntry>>,
}

impl ClassVerificationCache {
    /// Creates an empty whole-class proof memo.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reuses a proof only when its requested input and every observation remain exact.
    pub fn lookup<F>(
        &self,
        owner: &Arc<ClassDefinition>,
        request: ValueFingerprint,
        mut current: F,
    ) -> Option<Arc<ClassVerificationProof>>
    where
        F: FnMut(&ClassDefinitionId) -> Option<ValueFingerprint>,
    {
        let mut entries = self.entries();
        entries.retain(|_, entry| entry.owner.strong_count() != 0);
        let entry = entries.get(owner.id())?;
        if entry.request != request
            || entry
                .proof
                .dependencies
                .iter()
                .any(|observation| current(observation.key()) != observation.fingerprint())
        {
            return None;
        }
        Some(Arc::clone(&entry.proof))
    }

    /// Installs a proof under the managed class key without retaining that class.
    pub fn insert(
        &self,
        heap: &mut JvmHeap,
        cache: sim_lib_mutation::ManagedHandle,
        owner_handle: sim_lib_mutation::ManagedHandle,
        owner: &Arc<ClassDefinition>,
        request: ValueFingerprint,
        proof: ClassVerificationProof,
    ) -> Result<Arc<ClassVerificationProof>, JvmGraphError> {
        let managed_proof = heap.allocate(JvmRole::Cache).map_err(JvmGraphError::from)?;
        heap.ephemeron(cache, JvmEdge::DerivedEntry, owner_handle, managed_proof)?;
        let proof = Arc::new(proof);
        self.entries().insert(
            owner.id().clone(),
            ClassProofCacheEntry {
                owner: Arc::downgrade(owner),
                request,
                proof: Arc::clone(&proof),
                _managed_proof: managed_proof,
            },
        );
        Ok(proof)
    }

    /// Number of entries whose managed-class keys still exist.
    pub fn live_len(&self) -> usize {
        let mut entries = self.entries();
        entries.retain(|_, entry| entry.owner.strong_count() != 0);
        entries.len()
    }

    fn entries(&self) -> MutexGuard<'_, BTreeMap<ClassDefinitionId, ClassProofCacheEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Checks target declarations after the shared engine has joined all incoming states.
pub fn check_stack_map_constraints(
    classfile_version: u16,
    graph: &VerificationGraph,
    inferred: &BTreeMap<InstructionId, VerificationState>,
    declarations: &[ExpandedStackMapFrame],
    max_locals: usize,
    max_stack: usize,
) -> Result<(), StackMapConstraintError> {
    let declared: BTreeMap<_, _> = declarations
        .iter()
        .map(|frame| (frame.instruction, frame))
        .collect();
    let targets: std::collections::BTreeSet<_> = graph
        .edges()
        .filter(|edge| {
            matches!(
                edge.class(),
                EdgeClass::Custom(
                    VerificationEdgeClass::Branch | VerificationEdgeClass::Exceptional { .. }
                )
            )
        })
        .map(|edge| InstructionId(*edge.target()))
        .collect();
    for instruction in targets {
        let Some(frame) = declared.get(&instruction) else {
            if classfile_version >= 51 {
                return Err(StackMapConstraintError::Missing { instruction });
            }
            continue;
        };
        let state = inferred
            .get(&instruction)
            .ok_or(StackMapConstraintError::MissingInference { instruction })?;
        let declared_state = expanded_state(frame, max_locals, max_stack)
            .ok_or(StackMapConstraintError::NotAssignable { instruction })?;
        if !state.locals.less_equal(&declared_state.locals)
            || !state.stack.less_equal(&declared_state.stack)
            || stack_values(&state.stack).len() != stack_values(&declared_state.stack).len()
        {
            return Err(StackMapConstraintError::NotAssignable { instruction });
        }
    }
    Ok(())
}

fn expanded_state(
    frame: &ExpandedStackMapFrame,
    max_locals: usize,
    max_stack: usize,
) -> Option<VerificationState> {
    let mut locals = VerificationFrame::new(FrameKind::Locals, max_locals);
    let mut slot = 0;
    for value in &*frame.locals {
        locals.set_local(slot, value.clone()).ok()?;
        slot += type_width(value);
    }
    Some(VerificationState {
        locals,
        stack: stack_from_values(max_stack, frame.stack.to_vec()).ok()?,
    })
}
