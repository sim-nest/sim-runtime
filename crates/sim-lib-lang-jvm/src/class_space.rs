//! Authorized, lazy JVM class definition space.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};

use sim_codec_classfile::{ClassShell, Constant, ShellBudget};
use sim_kernel::{CapabilityName, CodecId, Cx, Dir, Error, Expr, Result, SourceId, Symbol};
use sim_lib_core::SourceAuthority;

static NEXT_LOADER_ID: AtomicU64 = AtomicU64::new(1);

/// Capability required before a JVM class source is read.
pub fn class_load_capability() -> CapabilityName {
    CapabilityName::new("jvm.class.load")
}

/// Stable identity of one class-loader namespace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClassLoaderId(u64);

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
}

/// An isolated, authority-requiring JVM class space.
pub struct ClassLoader {
    id: ClassLoaderId,
    definitions: Mutex<BTreeMap<String, Arc<ClassDefinition>>>,
    max_classfile_bytes: usize,
}

impl ClassLoader {
    /// Creates an empty loader with a process-local, non-reused identity.
    pub fn new(max_classfile_bytes: usize) -> Self {
        Self {
            id: ClassLoaderId(NEXT_LOADER_ID.fetch_add(1, Ordering::Relaxed)),
            definitions: Mutex::new(BTreeMap::new()),
            max_classfile_bytes,
        }
    }

    /// Returns this loader's isolated namespace identity.
    pub fn id(&self) -> ClassLoaderId {
        self.id
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
        let classfile = decode_named_class(
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
        let definition = Arc::new(ClassDefinition {
            id: ClassDefinitionId {
                loader: self.id,
                binary_name: request.binary_name.clone(),
                content_key,
            },
            classfile,
            content: bytes.into(),
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

fn decode_named_class(binary_name: &str, bytes: Vec<u8>, bound: usize) -> Result<Expr> {
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
    sim_codec_classfile::inspect_classfile(codec, bytes, bound)
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
}
