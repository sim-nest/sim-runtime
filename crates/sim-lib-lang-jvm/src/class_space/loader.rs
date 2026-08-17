/// Capability required before a JVM class source is read.
pub fn class_load_capability() -> CapabilityName {
    CapabilityName::new("jvm.class.load")
}

/// Stable identity of one class-loader namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClassLoaderId(pub(crate) u64);

/// A definition identity bound to loader namespace and exact classfile content.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClassDefinitionId {
    loader: ClassLoaderId,
    binary_name: String,
    content_key: u64,
}

impl ClassDefinitionId {
    /// Loader namespace owning this definition.
    pub fn loader(&self) -> ClassLoaderId {
        self.loader
    }
    /// Validated Java binary name.
    pub fn binary_name(&self) -> &str {
        &self.binary_name
    }
    /// Deterministic key derived from the complete decoded classfile input.
    pub fn content_key(&self) -> u64 {
        self.content_key
    }
}

/// A decoded class admitted into one loader namespace.
#[derive(Clone, Debug)]
pub struct ClassDefinition {
    id: ClassDefinitionId,
    classfile: Expr,
    content: Arc<[u8]>,
    metadata: Arc<crate::JavaClassMetadata>,
    literals: BTreeMap<u16, crate::JavaString>,
    resolution_records: BTreeMap<u16, crate::resolution::SymbolicConstant>,
    shell: Arc<ClassShell>,
}

impl ClassDefinition {
    #[cfg(test)]
    pub(crate) fn test(
        loader: ClassLoaderId,
        binary_name: &str,
        content_key: u64,
        metadata: crate::JavaClassMetadata,
        resolution_records: BTreeMap<u16, crate::resolution::SymbolicConstant>,
    ) -> Arc<Self> {
        Arc::new(Self {
            id: ClassDefinitionId {
                loader,
                binary_name: binary_name.into(),
                content_key,
            },
            classfile: Expr::Nil,
            content: Arc::from([]),
            metadata: Arc::new(metadata),
            literals: BTreeMap::new(),
            resolution_records,
            shell: Arc::new(empty_test_shell()),
        })
    }

    /// Content-bound definition identity.
    pub fn id(&self) -> &ClassDefinitionId {
        &self.id
    }

    #[cfg(test)]
    pub(crate) fn test_definition(metadata: crate::JavaClassMetadata) -> Self {
        let loader = metadata.resolution().loader();
        let binary_name = metadata.resolution().binary_name().to_owned();
        Self {
            id: ClassDefinitionId {
                loader,
                binary_name,
                content_key: 1,
            },
            classfile: Expr::Nil,
            content: Arc::from([]),
            metadata: Arc::new(metadata),
            literals: BTreeMap::new(),
            resolution_records: BTreeMap::new(),
            shell: Arc::new(empty_test_shell()),
        }
    }
    /// Retained, decoded classfile projection.
    pub fn classfile(&self) -> &Expr {
        &self.classfile
    }
    /// Neutral class face and retained JVM policy metadata.
    pub fn metadata(&self) -> &Arc<crate::JavaClassMetadata> {
        &self.metadata
    }
    /// Retained structural classfile shell used by bounded code browsing and execution.
    pub fn shell(&self) -> &ClassShell {
        &self.shell
    }
    /// Returns the interned Java string denoted by a `CONSTANT_String` index.
    pub fn string_literal(&self, constant_index: u16) -> Option<&crate::JavaString> {
        self.literals.get(&constant_index)
    }

    /// Returns the Java-visible class mirror for this definition.
    pub fn mirror(self: &Arc<Self>) -> crate::JavaClassMirror {
        crate::JavaClassMirror::new(self.clone())
    }

    pub(crate) fn resolution_record(
        &self,
        index: u16,
    ) -> Option<&crate::resolution::SymbolicConstant> {
        self.resolution_records.get(&index)
    }
}

/// An isolated, authority-requiring JVM class space.
pub struct ClassLoader {
    id: ClassLoaderId,
    revision: AtomicU64,
    definitions: Mutex<BTreeMap<String, Arc<ClassDefinition>>>,
    intern_pool: crate::text::JavaInternPool,
    max_classfile_bytes: usize,
}

impl ClassLoader {
    /// Creates an empty loader with a process-local, non-reused identity.
    pub fn new(max_classfile_bytes: usize) -> Self {
        Self::with_intern_limit(max_classfile_bytes, max_classfile_bytes.max(1))
    }

    /// Creates a loader with independent classfile-byte and intern-entry bounds.
    pub fn with_intern_limit(max_classfile_bytes: usize, max_interned_strings: usize) -> Self {
        Self {
            id: ClassLoaderId(NEXT_LOADER_ID.fetch_add(1, Ordering::Relaxed)),
            revision: AtomicU64::new(0),
            definitions: Mutex::new(BTreeMap::new()),
            intern_pool: crate::text::JavaInternPool::new(max_interned_strings),
            max_classfile_bytes,
        }
    }

    /// Returns this loader's isolated namespace identity.
    pub fn id(&self) -> ClassLoaderId {
        self.id
    }

    /// Returns the current identity of this loader's class-space contents.
    pub fn revision(&self) -> ClassSpaceRevision {
        ClassSpaceRevision {
            loader: self.id,
            revision: self.revision.load(Ordering::Acquire),
        }
    }

    #[cfg(test)]
    pub(crate) fn simulate_class_space_change(&self) {
        self.revision.fetch_add(1, Ordering::Release);
    }

    #[cfg(test)]
    pub(crate) fn test_insert(&self, definition: Arc<ClassDefinition>) {
        self.definitions
            .lock()
            .unwrap()
            .insert(definition.id.binary_name.clone(), definition);
    }

    #[cfg(test)]
    pub(crate) fn test_remove(&self, binary_name: &str) {
        self.definitions.lock().unwrap().remove(binary_name);
    }

    /// Interns exact code units in this loader's bounded literal namespace.
    pub fn intern(&self, units: &sim_text::CodeUnitString) -> Result<crate::JavaString> {
        self.intern_pool.intern(units)
    }

    /// Constructs a lazy request. No directory operation occurs until [`LazyClass::resolve`].
    pub fn request<'a>(
        &'a self,
        root_id: Symbol,
        root: Arc<dyn Dir>,
        binary_name: impl Into<String>,
        authority: SourceAuthority,
    ) -> Result<LazyClass<'a>> {
        let binary_name = binary_name.into();
        validate_binary_name(&binary_name)?;
        Ok(LazyClass {
            loader: self,
            root_id,
            root,
            binary_name,
            authority,
        })
    }

    /// Defines an explicitly supplied classfile after checking JVM load authority.
    ///
    /// Unlike [`Self::request`], this surface performs no directory or transport
    /// access: the caller supplies the complete, bounded byte string.
    pub fn define_bytes(
        &self,
        cx: &mut Cx,
        binary_name: impl Into<String>,
        bytes: Vec<u8>,
    ) -> Result<Arc<ClassDefinition>> {
        cx.require(&class_load_capability())?;
        let binary_name = binary_name.into();
        validate_binary_name(&binary_name)?;
        self.define_decoded(cx, binary_name, bytes)
    }

    /// Returns at most `limit` loaded definitions in binary-name order.
    pub fn browse_classes(&self, limit: usize) -> Result<Vec<Arc<ClassDefinition>>> {
        Ok(self.definitions()?.values().take(limit).cloned().collect())
    }

    fn definitions(&self) -> Result<MutexGuard<'_, BTreeMap<String, Arc<ClassDefinition>>>> {
        self.definitions
            .lock()
            .map_err(|_| Error::Eval("JVM class space lock poisoned".into()))
    }

    fn resolve(&self, cx: &mut Cx, request: &LazyClass<'_>) -> Result<Arc<ClassDefinition>> {
        // Every authority check precedes the first call into the supplied directory.
        cx.require(&class_load_capability())?;
        for capability in request.authority.requires() {
            cx.require(capability)?;
        }

        let path = format!("{}.class", request.binary_name.replace('.', "/"));
        let bytes = read_bytes(cx, request.root.as_ref(), &path)?;
        if bytes.len() > self.max_classfile_bytes {
            return Err(Error::Eval(format!(
                "classfile exceeds {} byte allowance",
                self.max_classfile_bytes
            )));
        }
        self.define_decoded(cx, request.binary_name.clone(), bytes)
    }

    fn define_decoded(
        &self,
        cx: &mut Cx,
        binary_name: String,
        bytes: Vec<u8>,
    ) -> Result<Arc<ClassDefinition>> {
        if bytes.len() > self.max_classfile_bytes {
            return Err(Error::Eval(format!(
                "classfile exceeds {} byte allowance",
                self.max_classfile_bytes
            )));
        }
        let (classfile, shell, validated) =
            decode_named_class(&binary_name, bytes.clone(), self.max_classfile_bytes)?;
        let content_key = content_key(&bytes);
        let mut definitions = self.definitions()?;
        if let Some(existing) = definitions.get(&binary_name) {
            if existing.content.as_ref() == bytes.as_slice() {
                return Ok(existing.clone());
            }
            return Err(Error::Eval(format!(
                "duplicate definition of {} in loader {}",
                binary_name, self.id.0
            )));
        }
        let id = ClassDefinitionId {
            loader: self.id,
            binary_name: binary_name.clone(),
            content_key,
        };
        let metadata = Arc::new(crate::JavaClassMetadata::from_shell(
            cx, &id, &shell, &validated,
        )?);
        let mut literals = BTreeMap::new();
        for (offset, constant) in shell.constant_pool.slots().iter().enumerate() {
            let ConstantSlot::Entry(Constant::String { string_index }) = constant else {
                continue;
            };
            let Constant::Utf8(units) = shell
                .constant_pool
                .entry(*string_index, *string_index)
                .map_err(|error| Error::Eval(error.to_string()))?
            else {
                return Err(Error::Eval(format!(
                    "constant #{string_index} is not UTF-8"
                )));
            };
            let index = u16::try_from(offset)
                .map_err(|_| Error::Eval("constant-pool index overflow".into()))?;
            literals.insert(index, self.intern(units)?);
        }
        let resolution_records = crate::resolution::symbolic_constants(&shell)?;
        let definition = Arc::new(ClassDefinition {
            id,
            classfile,
            content: bytes.into(),
            metadata,
            literals,
            resolution_records,
            shell: Arc::new(shell),
        });
        definitions.insert(binary_name, definition.clone());
        self.revision.fetch_add(1, Ordering::Release);
        Ok(definition)
    }

    /// Returns a currently loaded definition without initiating class loading.
    pub fn loaded(&self, binary_name: &str) -> Result<Option<Arc<ClassDefinition>>> {
        Ok(self.definitions()?.get(binary_name).cloned())
    }
}

#[cfg(test)]
fn empty_test_shell() -> ClassShell {
    decode_named_class(
        "Minimal",
        include_bytes!("../../fixtures/hand-built/Minimal.class").to_vec(),
        4096,
    )
    .expect("checked JVM fixture")
    .1
}
