/// Identity of one exact class-space state, not a mutable validity flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClassSpaceRevision {
    loader: ClassLoaderId,
    revision: u64,
}
impl ClassSpaceRevision {
    /// Loader namespace whose state this revision identifies.
    pub const fn loader(self) -> ClassLoaderId {
        self.loader
    }

    /// Monotonic revision within the loader namespace.
    pub const fn number(self) -> u64 {
        self.revision
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
