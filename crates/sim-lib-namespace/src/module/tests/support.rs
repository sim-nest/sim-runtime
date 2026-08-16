#[derive(Default)]
struct MemoryDir {
    files: RwLock<BTreeMap<Symbol, Value>>,
    dirs: RwLock<BTreeMap<Symbol, Arc<MemoryDir>>>,
}
impl MemoryDir {
    fn directory(&self, cx: &mut Cx, name: &str) -> Arc<Self> {
        let dir = Arc::new(Self::default());
        self.dirs
            .write()
            .unwrap()
            .insert(Symbol::new(name), dir.clone());
        let _ = cx;
        dir
    }

    fn source(&self, cx: &mut Cx, name: &str, source: &str) {
        self.files.write().unwrap().insert(
            Symbol::new(name),
            cx.factory().string(source.to_owned()).unwrap(),
        );
    }
}

impl Object for MemoryDir {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("memory-module-root".to_owned())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for MemoryDir {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory()
            .class_stub(ClassId(0), Symbol::qualified("test", "ModuleRoot"))
    }
    fn as_table_impl(&self) -> Option<&dyn Table> {
        Some(self)
    }
    fn as_dir(&self) -> Option<&dyn Dir> {
        Some(self)
    }
}

impl Table for MemoryDir {
    fn backend_symbol(&self) -> Symbol {
        Symbol::qualified("test", "module-root")
    }
    fn get(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        self.files
            .read()
            .unwrap()
            .get(&key)
            .cloned()
            .map_or_else(|| cx.factory().nil(), Ok)
    }
    fn set(&self, _cx: &mut Cx, key: Symbol, value: Value) -> Result<()> {
        self.files.write().unwrap().insert(key, value);
        Ok(())
    }
    fn has(&self, _cx: &mut Cx, key: Symbol) -> Result<bool> {
        Ok(self.files.read().unwrap().contains_key(&key))
    }
    fn del(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
        self.files
            .write()
            .unwrap()
            .remove(&key)
            .map_or_else(|| cx.factory().nil(), Ok)
    }
    fn keys(&self, _cx: &mut Cx) -> Result<Vec<Symbol>> {
        Ok(self.files.read().unwrap().keys().cloned().collect())
    }
    fn entries(&self, _cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
        Ok(self
            .files
            .read()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }
    fn len(&self, _cx: &mut Cx) -> Result<usize> {
        Ok(self.files.read().unwrap().len())
    }
    fn clear(&self, _cx: &mut Cx) -> Result<()> {
        self.files.write().unwrap().clear();
        Ok(())
    }
}

impl Dir for MemoryDir {
    fn mkdir(&self, cx: &mut Cx, name: Symbol) -> Result<Value> {
        let dir = self.directory(cx, &name.name);
        cx.factory().opaque(dir)
    }
    fn opendir(&self, cx: &mut Cx, name: Symbol) -> Result<Option<Value>> {
        self.dirs
            .read()
            .unwrap()
            .get(&name)
            .cloned()
            .map(|dir| cx.factory().opaque(dir))
            .transpose()
    }
    fn rmdir(&self, cx: &mut Cx, name: Symbol) -> Result<Value> {
        self.dirs
            .write()
            .unwrap()
            .remove(&name)
            .map_or_else(|| cx.factory().nil(), |dir| cx.factory().opaque(dir))
    }
    fn is_dir(&self, _cx: &mut Cx, name: Symbol) -> Result<bool> {
        Ok(self.dirs.read().unwrap().contains_key(&name))
    }
}

fn context() -> Cx {
    let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    seat.grant(&mut cx, module_load_capability()).unwrap();
    seat.grant(&mut cx, read_eval_capability()).unwrap();
    cx.load_lib(&LispCodecLib::new(sim_kernel::CodecId(31)).unwrap())
        .unwrap();
    cx
}

fn request(
    root: Arc<MemoryDir>,
    specifier: &str,
    importer: Option<ModuleIdentity>,
) -> ModuleRequest {
    ModuleRequest::new(
        Symbol::new("fixture"),
        root,
        importer,
        specifier.to_owned(),
        Symbol::qualified("codec", "lisp"),
        SourceAuthority::new(
            ReadPolicy {
                trust: TrustLevel::TrustedSource,
                capabilities: CapabilitySet::new().grant(read_eval_capability()),
            },
            vec![module_load_capability()],
            CapabilitySet::new()
                .grant(read_eval_capability())
                .grant(module_load_capability()),
        )
        .unwrap(),
    )
}

fn value_expr(cx: &mut Cx, module: &ModuleInstance) -> Expr {
    module
        .default_export()
        .get()
        .unwrap()
        .object()
        .as_expr(cx)
        .unwrap()
}

#[test]
fn relative_resolution_and_root_escape_are_explicit() {
    let mut cx = context();
    let root = Arc::new(MemoryDir::default());
    let pkg = root.directory(&mut cx, "pkg");
    pkg.source(&mut cx, "sibling.sim", "\"relative\"");
    let importer = ModuleIdentity {
        root: Symbol::new("fixture"),
        path: "pkg/main.sim".to_owned(),
    };
    let loader = ModuleLoader::new();
    let loaded = loader
        .load(
            &mut cx,
            request(root.clone(), "./sibling.sim", Some(importer.clone())),
        )
        .unwrap();
    assert_eq!(loaded.identity.path(), "pkg/sibling.sim");
    assert_eq!(
        value_expr(&mut cx, &loaded),
        Expr::String("relative".to_owned())
    );
    let error = loader
        .load(&mut cx, request(root, "../../escape.sim", Some(importer)))
        .unwrap_err();
    assert!(error.to_string().contains("escapes supplied root"));
}

#[test]
fn identity_specifier_policy_preserves_every_landed_specifier_byte_for_byte() {
    let importer = ModuleIdentity {
        root: Symbol::new("fixture"),
        path: "pkg/main.sim".to_owned(),
    };
    let policy = IdentitySpecifierPolicy;
    for specifier in [
        "plain.sim",
        "pkg/nested.sim",
        "./sibling.sim",
        "../parent.sim",
        "with spaces.sim",
        "unicodé.sim",
    ] {
        let request =
            SpecifierPolicyRequest::new(Some(importer.clone()), vec![specifier.to_owned()])
                .unwrap();
        let normalized = policy.resolve(&request).unwrap();
        assert_eq!(normalized.as_str().as_bytes(), specifier.as_bytes());
    }
}

#[test]
fn identity_policy_refuses_fallback_and_all_policy_inputs_are_bounded() {
    let policy = IdentitySpecifierPolicy;
    let alternatives = SpecifierPolicyRequest::new(
        None,
        vec!["module.sim".to_owned(), "module/index.sim".to_owned()],
    )
    .unwrap();
    let refusal = policy.resolve(&alternatives).unwrap_err();
    assert_eq!(
        refusal.code(),
        SpecifierRefusalCode::IdentityRequiresOneCandidate
    );
    assert_eq!(
        refusal.detail(),
        "identity module specifier policy requires exactly one candidate, got 2"
    );

    assert_eq!(
        SpecifierPolicyRequest::new(None, Vec::new())
            .unwrap_err()
            .code(),
        SpecifierRefusalCode::NoCandidates
    );
    assert_eq!(
        SpecifierPolicyRequest::new(
            None,
            (0..=MAX_SPECIFIER_CANDIDATES)
                .map(|index| format!("{index}.sim"))
                .collect(),
        )
        .unwrap_err()
        .code(),
        SpecifierRefusalCode::TooManyCandidates
    );
    assert_eq!(
        SpecifierPolicyRequest::new(None, vec!["x".repeat(MAX_SPECIFIER_BYTES + 1)])
            .unwrap_err()
            .code(),
        SpecifierRefusalCode::CandidateTooLong
    );
}
