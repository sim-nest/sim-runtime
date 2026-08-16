//! Authorized, lazy JVM class definition space.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};

use sim_codec_classfile::{ClassShell, Constant, ConstantSlot, ShellBudget};
use sim_kernel::{CapabilityName, CodecId, Cx, Dir, Error, Expr, Result, SourceId, Symbol};
use sim_lib_core::SourceAuthority;

static NEXT_LOADER_ID: AtomicU64 = AtomicU64::new(1);

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
}

impl ClassDefinition {
    /// Content-bound definition identity.
    pub fn id(&self) -> &ClassDefinitionId {
        &self.id
    }
    /// Retained, decoded classfile projection.
    pub fn classfile(&self) -> &Expr {
        &self.classfile
    }
    /// Neutral class face and retained JVM policy metadata.
    pub fn metadata(&self) -> &Arc<crate::JavaClassMetadata> {
        &self.metadata
    }
    /// Returns the interned Java string denoted by a `CONSTANT_String` index.
    pub fn string_literal(&self, constant_index: u16) -> Option<&crate::JavaString> {
        self.literals.get(&constant_index)
    }

    /// Returns the Java-visible class mirror for this definition.
    pub fn mirror(self: &Arc<Self>) -> crate::JavaClassMirror {
        crate::JavaClassMirror::new(self.clone())
    }
}

/// An isolated, authority-requiring JVM class space.
pub struct ClassLoader {
    id: ClassLoaderId,
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
            definitions: Mutex::new(BTreeMap::new()),
            intern_pool: crate::text::JavaInternPool::new(max_interned_strings),
            max_classfile_bytes,
        }
    }

    /// Returns this loader's isolated namespace identity.
    pub fn id(&self) -> ClassLoaderId {
        self.id
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
        let (classfile, shell, validated) = decode_named_class(
            &request.binary_name,
            bytes.clone(),
            self.max_classfile_bytes,
        )?;
        let content_key = content_key(&bytes);
        let mut definitions = self.definitions()?;
        if let Some(existing) = definitions.get(&request.binary_name) {
            if existing.content.as_ref() == bytes.as_slice() {
                return Ok(existing.clone());
            }
            return Err(Error::Eval(format!(
                "duplicate definition of {} in loader {}",
                request.binary_name, self.id.0
            )));
        }
        let id = ClassDefinitionId {
            loader: self.id,
            binary_name: request.binary_name.clone(),
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
        let definition = Arc::new(ClassDefinition {
            id,
            classfile,
            content: bytes.into(),
            metadata,
            literals,
        });
        definitions.insert(request.binary_name.clone(), definition.clone());
        Ok(definition)
    }
}

/// A class request retaining the only root and authority that may resolve it.
pub struct LazyClass<'a> {
    loader: &'a ClassLoader,
    root_id: Symbol,
    root: Arc<dyn Dir>,
    binary_name: String,
    authority: SourceAuthority,
}

impl LazyClass<'_> {
    /// Caller-assigned identity of the sole visible source root.
    pub fn root_id(&self) -> &Symbol {
        &self.root_id
    }
    /// Resolves and defines the class on first demand.
    pub fn resolve(&self, cx: &mut Cx) -> Result<Arc<ClassDefinition>> {
        self.loader.resolve(cx, self)
    }
}

fn validate_binary_name(name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('.') || name.ends_with('.') || name.contains("..") {
        return Err(Error::Eval(format!("invalid JVM binary name: {name}")));
    }
    if name.split('.').any(|part| {
        let mut chars = part.chars();
        !chars
            .next()
            .is_some_and(|c| c == '_' || c == '$' || c.is_alphabetic())
            || chars.any(|c| !(c == '_' || c == '$' || c.is_alphanumeric()))
    }) {
        return Err(Error::Eval(format!("invalid JVM binary name: {name}")));
    }
    Ok(())
}

fn read_bytes(cx: &mut Cx, root: &dyn Dir, path: &str) -> Result<Vec<u8>> {
    let components: Vec<_> = path.split('/').collect();
    read_components(cx, root, &components, path)
}

fn read_components(cx: &mut Cx, dir: &dyn Dir, components: &[&str], path: &str) -> Result<Vec<u8>> {
    let (component, rest) = components.split_first().expect("class path has a file");
    if !rest.is_empty() {
        let value = dir
            .opendir(cx, Symbol::new(*component))?
            .ok_or_else(|| Error::Eval(format!("class directory not found: {path}")))?;
        let child = value.object().as_dir().ok_or_else(|| {
            Error::Eval(format!("class path component is not a directory: {path}"))
        })?;
        return read_components(cx, child, rest, path);
    }
    let key = Symbol::new(*component);
    if !dir.has(cx, key.clone())? {
        return Err(Error::Eval(format!("class source not found: {path}")));
    }
    match dir.get(cx, key)?.object().as_expr(cx)? {
        Expr::Bytes(bytes) => Ok(bytes),
        _ => Err(Error::Eval(format!("class source is not bytes: {path}"))),
    }
}

fn decode_named_class(
    binary_name: &str,
    bytes: Vec<u8>,
    bound: usize,
) -> Result<(Expr, ClassShell, sim_codec_classfile::ValidatedClassShell)> {
    let codec = CodecId(139);
    let budget = ShellBudget {
        interfaces: bound,
        fields: bound,
        methods: bound,
        attributes: bound,
        attribute_bytes: bound,
    };
    let shell = ClassShell::decode(
        &bytes,
        bound.saturating_mul(4).max(1024),
        budget,
        codec,
        SourceId("jvm-class-loader".into()),
    )
    .map_err(|error| Error::Eval(error.to_string()))?;
    let validated = shell
        .validate()
        .map_err(|error| Error::Eval(error.to_string()))?;
    let Constant::Class { name_index } = shell
        .constant_pool
        .entry(validated.this_class.0, validated.this_class.0)
        .map_err(|error| Error::Eval(error.to_string()))?
    else {
        return Err(Error::Eval("this_class is not a class constant".into()));
    };
    let Constant::Utf8(name) = shell
        .constant_pool
        .entry(*name_index, *name_index)
        .map_err(|error| Error::Eval(error.to_string()))?
    else {
        return Err(Error::Eval("class name is not UTF-8".into()));
    };
    let internal = String::from_utf16(name.as_code_units())
        .map_err(|_| Error::Eval("class name is not valid Unicode".into()))?;
    if internal != binary_name.replace('.', "/") {
        return Err(Error::Eval(format!(
            "requested {binary_name}, classfile defines {}",
            internal.replace('/', ".")
        )));
    }
    let projection = sim_codec_classfile::inspect_classfile(codec, bytes, bound)?;
    Ok((projection, shell, validated))
}

fn content_key(bytes: &[u8]) -> u64 {
    // FNV-1a is used as a deterministic content key, not as a security boundary.
    bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use sim_kernel::{
        CapabilitySet, ClassId, ClassRef, DefaultFactory, EagerPolicy, Object, ObjectCompat,
        ReadPolicy, Table, TrustLevel, Value, read_eval_capability,
    };

    use super::*;

    struct FixtureDir {
        bytes: Vec<u8>,
        reads: AtomicUsize,
    }

    impl FixtureDir {
        fn new() -> Self {
            Self {
                bytes: include_bytes!("../fixtures/hand-built/Minimal.class").to_vec(),
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
        let mut bytes = include_bytes!("../fixtures/hand-built/Minimal.class").to_vec();
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
}
