/// Java-permitted identity policy for a non-capturing lambda site.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatelessLambdaIdentity {
    /// Allocate on every factory call. This is always valid Java behavior.
    #[default]
    Fresh,
    /// Reuse one instance. Java permits this only for a non-capturing site.
    PermittedSingleton,
}
/// One linked, loader-owned lambda factory.
pub struct ManagedLambdaFactory {
    class: Arc<GeneratedLambdaClass>,
    plan: JvmFunctionPlan,
    class_value: ClassRef,
    managed: ManagedHandle,
    identity: StatelessLambdaIdentity,
    singleton: Option<(Arc<FunctionInstance<JvmFunctionPolicyBody>>, ManagedHandle)>,
}

impl ManagedLambdaFactory {
    /// Managed factory node stored as the value of the site ephemeron.
    pub const fn managed(&self) -> ManagedHandle {
        self.managed
    }

    /// Generated class owned by this factory.
    pub fn generated_class(&self) -> &Arc<GeneratedLambdaClass> {
        &self.class
    }

    /// Allocates an instance with captures in exact frozen-plan order.
    pub fn instantiate(
        &mut self,
        heap: &mut JvmHeap,
        captures: Vec<CapturedBinding>,
    ) -> Result<ManagedLambdaInstance, LambdaFactoryError> {
        if captures.len() != self.plan.neutral().captures().len() {
            return Err(LambdaFactoryError::CaptureArity {
                expected: self.plan.neutral().captures().len(),
                actual: captures.len(),
            });
        }
        if captures.is_empty()
            && self.identity == StatelessLambdaIdentity::PermittedSingleton
            && let Some((function, managed)) = &self.singleton
        {
            let root = heap.root(*managed).map_err(LambdaFactoryError::managed)?;
            return Ok(ManagedLambdaInstance {
                function: function.clone(),
                managed: *managed,
                root,
            });
        }
        let function = Arc::new(
            FunctionInstance::new(
                self.plan.neutral().clone(),
                self.plan.body().clone(),
                captures,
                self.class_value.clone(),
                None,
                None,
            )
            .map_err(|error| LambdaFactoryError::Instance(error.to_string()))?,
        );
        let managed = heap
            .allocate(crate::JvmRole::Object)
            .map_err(LambdaFactoryError::managed)?;
        heap.strong(managed, crate::JvmEdge::Class, self.class.mirror())
            .map_err(LambdaFactoryError::graph)?;
        for capture in function.captures() {
            heap.strong(managed, crate::JvmEdge::Field, capture.managed())
                .map_err(LambdaFactoryError::graph)?;
        }
        if function.captures().is_empty()
            && self.identity == StatelessLambdaIdentity::PermittedSingleton
        {
            heap.strong(self.managed, crate::JvmEdge::Field, managed)
                .map_err(LambdaFactoryError::graph)?;
            self.singleton = Some((function.clone(), managed));
        }
        let root = heap.root(managed).map_err(LambdaFactoryError::managed)?;
        Ok(ManagedLambdaInstance {
            function,
            managed,
            root,
        })
    }
}

/// A rooted managed lease for one lambda object.
pub struct ManagedLambdaInstance {
    function: Arc<FunctionInstance<JvmFunctionPolicyBody>>,
    managed: ManagedHandle,
    root: RootedHandle,
}

/// Located refusal to manufacture a Java serialized-lambda replacement.
///
/// SIM does not invoke host Java serialization or serialize the Rust function
/// object. A replacement can be admitted only after the JVM language library
/// owns exact managed `SerializedLambda` data and an authorized, validating
/// read-resolution protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LambdaSerializationError {
    /// The bootstrap did not declare this generated class serializable.
    NotDeclared {
        /// Capturing loader identity.
        loader: crate::ClassLoaderId,
        /// Generated class identity.
        class: String,
        /// Managed lambda object on which replacement was requested.
        object: ManagedHandle,
    },
    /// Serialization was declared, but the exact managed replacement/read
    /// protocol is not present and no weaker host mechanism is permitted.
    ManagedReplacementUnavailable {
        /// Capturing loader identity.
        loader: crate::ClassLoaderId,
        /// Generated class identity.
        class: String,
        /// Managed lambda object on which replacement was requested.
        object: ManagedHandle,
    },
}

impl ManagedLambdaInstance {
    /// Neutral function object carrying the exact capture cells.
    pub fn function(&self) -> &Arc<FunctionInstance<JvmFunctionPolicyBody>> {
        &self.function
    }

    /// Managed JVM object identity.
    pub const fn managed(&self) -> ManagedHandle {
        self.managed
    }

    /// Refuses serialization until the exact managed replacement protocol exists.
    ///
    /// This is deliberately a located runtime failure instead of an opaque
    /// omission. In particular, this method never consults a host JVM, a host
    /// serializer, or the captured Rust [`FunctionInstance`].
    pub fn serialized_replacement(
        &self,
        class: &GeneratedLambdaClass,
    ) -> Result<std::convert::Infallible, LambdaSerializationError> {
        let location = || (class.loader(), class.binary_name().to_owned(), self.managed);
        if class.serializable() {
            let (loader, class, object) = location();
            Err(LambdaSerializationError::ManagedReplacementUnavailable {
                loader,
                class,
                object,
            })
        } else {
            let (loader, class, object) = location();
            Err(LambdaSerializationError::NotDeclared {
                loader,
                class,
                object,
            })
        }
    }

    /// Releases this explicit heap root.
    pub fn release(self, heap: &mut JvmHeap) -> Result<(), LambdaFactoryError> {
        heap.release_root(self.root)
            .map_err(LambdaFactoryError::managed)?;
        Ok(())
    }
}

struct LambdaFactoryEntry {
    owner: Weak<ClassDefinition>,
    factory: Arc<std::sync::Mutex<ManagedLambdaFactory>>,
}

/// Occurrence-keyed factory cache whose managed entries are owner ephemerons.
#[derive(Default)]
pub struct LambdaFactoryCache {
    entries: BTreeMap<SiteKey, LambdaFactoryEntry>,
}

impl LambdaFactoryCache {
    /// Returns the existing factory for a live site or installs one.
    #[allow(clippy::too_many_arguments)]
    pub fn link(
        &mut self,
        heap: &mut JvmHeap,
        cache: ManagedHandle,
        owner_handle: ManagedHandle,
        owner: &Arc<ClassDefinition>,
        site: SiteKey,
        class: Arc<GeneratedLambdaClass>,
        plan: JvmFunctionPlan,
        class_value: ClassRef,
        identity: StatelessLambdaIdentity,
    ) -> Result<Arc<std::sync::Mutex<ManagedLambdaFactory>>, LambdaFactoryError> {
        self.entries
            .retain(|_, entry| entry.owner.strong_count() != 0);
        if !plan.neutral().captures().is_empty()
            && identity == StatelessLambdaIdentity::PermittedSingleton
        {
            return Err(LambdaFactoryError::CapturingSingleton);
        }
        if let Some(entry) = self.entries.get(&site) {
            return Ok(entry.factory.clone());
        }
        let managed = heap
            .allocate(crate::JvmRole::Object)
            .map_err(LambdaFactoryError::managed)?;
        heap.strong(managed, crate::JvmEdge::Class, class.mirror())
            .map_err(LambdaFactoryError::graph)?;
        heap.ephemeron(cache, crate::JvmEdge::DerivedEntry, owner_handle, managed)
            .map_err(LambdaFactoryError::graph)?;
        let factory = Arc::new(std::sync::Mutex::new(ManagedLambdaFactory {
            class,
            plan,
            class_value,
            managed,
            identity,
            singleton: None,
        }));
        self.entries.insert(
            site,
            LambdaFactoryEntry {
                owner: Arc::downgrade(owner),
                factory: factory.clone(),
            },
        );
        Ok(factory)
    }

    /// Number of entries whose capturing class loader remains live.
    pub fn live_len(&mut self) -> usize {
        self.entries
            .retain(|_, entry| entry.owner.strong_count() != 0);
        self.entries.len()
    }
}

/// Failure to link a factory or allocate a lambda instance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LambdaFactoryError {
    /// Captures did not exactly fill the frozen neutral plan.
    CaptureArity {
        /// Frozen capture-slot count.
        expected: usize,
        /// Supplied capture-cell count.
        actual: usize,
    },
    /// Singleton reuse is forbidden for capturing lambdas.
    CapturingSingleton,
    /// Neutral function construction failed.
    Instance(String),
    /// Managed allocation or rooting failed.
    Managed(String),
    /// Managed edge construction failed.
    Graph(String),
}

impl LambdaFactoryError {
    fn managed(error: impl std::fmt::Debug) -> Self {
        Self::Managed(format!("{error:?}"))
    }

    fn graph(error: impl std::fmt::Debug) -> Self {
        Self::Graph(format!("{error:?}"))
    }
}

/// Failure to assemble a managed byte-free lambda class.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GeneratedLambdaClassError {
    /// Checked neutral metadata rejected the generated declaration.
    Metadata(String),
    /// Managed class-mirror allocation failed.
    Managed(String),
}

fn generated_identity(
    loader: crate::ClassLoaderId,
    name: &str,
    fingerprint: u64,
) -> Result<ClassIdentity, GeneratedLambdaClassError> {
    let folded = fingerprint ^ loader.0 ^ (loader.0.rotate_left(23));
    let raw = ((folded >> 32) as u32 ^ folded as u32).max(1);
    ClassIdentity::checked(ClassId(raw), Symbol::new(name.to_owned()))
        .map_err(|error| GeneratedLambdaClassError::Metadata(error.to_string()))
}

fn lambda_site_fingerprint(site: &SiteKey) -> u64 {
    let mut hash = stable_text_hash(site.class.binary_name());
    hash = stable_hash_bytes(hash, &site.class.content_key().to_le_bytes());
    hash = stable_hash_bytes(hash, site.method.name.as_bytes());
    hash = stable_hash_bytes(hash, site.method.descriptor.as_bytes());
    stable_hash_bytes(hash, &site.constant_pool_index.to_le_bytes())
}

fn stable_text_hash(value: &str) -> u64 {
    stable_hash_bytes(0xcbf29ce484222325, value.as_bytes())
}

fn stable_hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    hash
}

/// Fail-closed lambda bootstrap admission or payload error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LambdaBootstrapError {
    /// Bootstrap identity is absent from the shared registry.
    UnadmittedProtocol,
    /// Payload length, kind, flags, count, or descriptor is malformed.
    MalformedPayload(String),
    /// Implementation handle is not an invocable method handle.
    UnadmittedReferenceKind(u8),
}
