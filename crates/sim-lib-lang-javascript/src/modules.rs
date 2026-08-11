//! ECMAScript source modules and dynamic source under explicit authority.

use std::sync::Arc;

use sim_kernel::{
    CapabilityName, CapabilitySet, Cx, Dir, ReadPolicy, Result, Shape, Symbol, Value,
};
use sim_lib_core::{ReadEvalBroker, ReadEvalRequest, ReadEvalSource, RequestOrigin};
use sim_lib_namespace::{
    ModuleIdentity, ModuleInstance, ModuleLoader, ModuleRequest, ModuleResolutionReceipt,
};
use sim_shape::AnyShape;

/// JavaScript policy over the shared parse/link/evaluate module lifecycle.
///
/// The shared loader canonicalizes supplied-root identities, detects cycles,
/// caches failures, and exposes its default export through a live binding cell.
pub struct JavascriptModulePolicy {
    loader: ModuleLoader,
    codec: Symbol,
}

impl Default for JavascriptModulePolicy {
    fn default() -> Self {
        Self::with_codec(Symbol::qualified("codec", "javascript"))
    }
}

impl JavascriptModulePolicy {
    /// Builds module policy for an installed compatible source codec.
    pub fn with_codec(codec: Symbol) -> Self {
        Self {
            loader: ModuleLoader::new(),
            codec,
        }
    }

    /// Parses, links, and evaluates an ESM source from the supplied root.
    pub fn load(
        &self,
        cx: &mut Cx,
        specifier: impl Into<String>,
        admission: JavascriptModuleAdmission,
    ) -> Result<ModuleInstance> {
        self.load_from(cx, None, specifier, admission)
    }

    /// Resolves a static import relative to an already linked module.
    pub fn load_from(
        &self,
        cx: &mut Cx,
        importer: Option<ModuleIdentity>,
        specifier: impl Into<String>,
        admission: JavascriptModuleAdmission,
    ) -> Result<ModuleInstance> {
        self.loader.load(
            cx,
            ModuleRequest {
                root_id: admission.root_id,
                root: admission.root,
                importer,
                specifier: specifier.into(),
                codec: self.codec.clone(),
                read_policy: admission.read_policy,
                requires: admission.requires,
                allow: admission.allow,
            },
        )
    }

    /// Dynamic import uses the same lifecycle and supplied-root envelope.
    pub fn dynamic_import(
        &self,
        cx: &mut Cx,
        importer: Option<ModuleIdentity>,
        specifier: impl Into<String>,
        admission: JavascriptModuleAdmission,
    ) -> Result<ModuleInstance> {
        self.load_from(cx, importer, specifier, admission)
    }

    /// Ordered link, cache-hit, cycle, and failure evidence.
    pub fn receipts(&self) -> Result<Vec<ModuleResolutionReceipt>> {
        self.loader.receipts()
    }
}

/// Host-authored root and authority supplied for one ESM resolution.
pub struct JavascriptModuleAdmission {
    /// Stable identity for the supplied module root.
    pub root_id: Symbol,
    /// The only directory visible to module resolution.
    pub root: Arc<dyn Dir>,
    /// Trusted policy used by diminished read-eval.
    pub read_policy: ReadPolicy,
    /// Powers the importing caller must already hold.
    pub requires: Vec<CapabilityName>,
    /// Diminished powers visible during source evaluation.
    pub allow: CapabilitySet,
}

/// Capability-gated JavaScript `eval`/`Function` source entry.
pub struct DynamicJavascript {
    broker: ReadEvalBroker,
    codec: Symbol,
}

impl Default for DynamicJavascript {
    fn default() -> Self {
        Self::with_codec(Symbol::qualified("codec", "javascript"))
    }
}

impl DynamicJavascript {
    /// Builds dynamic source policy for an installed compatible codec.
    pub fn with_codec(codec: Symbol) -> Self {
        Self {
            broker: ReadEvalBroker::new(),
            codec,
        }
    }

    /// Evaluates dynamic text only through diminished read-eval.
    pub fn evaluate(
        &self,
        cx: &mut Cx,
        source: impl Into<String>,
        admission: JavascriptDynamicAdmission,
    ) -> Result<Value> {
        self.broker.admit(
            cx,
            ReadEvalRequest {
                origin: RequestOrigin::new(Symbol::qualified("javascript", "dynamic-source")),
                codec: self.codec.clone(),
                source: ReadEvalSource::Text(source.into()),
                read_policy: admission.read_policy,
                requires: admission.requires,
                allow: admission.allow,
                expected_shape: admission.expected_shape,
            },
        )
    }
}

/// Host-authored authority envelope for dynamic JavaScript source.
pub struct JavascriptDynamicAdmission {
    /// Trusted read policy; source cannot construct it.
    pub read_policy: ReadPolicy,
    /// Powers the caller must already hold.
    pub requires: Vec<CapabilityName>,
    /// Diminished powers visible during evaluation.
    pub allow: CapabilitySet,
    /// Required result shape.
    pub expected_shape: Arc<dyn Shape>,
}

impl JavascriptDynamicAdmission {
    /// Builds an envelope with no ambient capabilities and an unconstrained result.
    pub fn new(read_policy: ReadPolicy) -> Self {
        Self {
            read_policy,
            requires: Vec::new(),
            allow: CapabilitySet::new(),
            expected_shape: Arc::new(AnyShape),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::RwLock};

    use sim_codec_lisp::LispCodecLib;
    use sim_kernel::{
        ClassId, ClassRef, CodecId, DefaultFactory, EagerPolicy, Error, Object, ObjectCompat,
        Table, TrustLevel, read_eval_capability,
    };
    use sim_lib_namespace::{ModuleResolutionOutcome, module_load_capability};

    use super::*;

    fn context() -> (Cx, sim_kernel::GrantSeat) {
        let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
        cx.load_lib(&LispCodecLib::new(CodecId(71)).unwrap())
            .unwrap();
        (cx, seat)
    }

    fn trusted() -> ReadPolicy {
        ReadPolicy {
            trust: TrustLevel::TrustedSource,
            capabilities: CapabilitySet::new().grant(read_eval_capability()),
        }
    }

    #[derive(Default)]
    struct MemoryDir(RwLock<BTreeMap<Symbol, Value>>);

    impl MemoryDir {
        fn source(&self, cx: &mut Cx, name: &str, source: &str) {
            self.0.write().unwrap().insert(
                Symbol::new(name),
                cx.factory().string(source.to_owned()).unwrap(),
            );
        }
    }

    impl Object for MemoryDir {
        fn display(&self, _cx: &mut Cx) -> Result<String> {
            Ok("javascript-memory-root".to_owned())
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    impl ObjectCompat for MemoryDir {
        fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
            cx.factory()
                .class_stub(ClassId(0), Symbol::qualified("test", "JavascriptRoot"))
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
            Symbol::qualified("test", "javascript-root")
        }
        fn get(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
            self.0
                .read()
                .unwrap()
                .get(&key)
                .cloned()
                .map_or_else(|| cx.factory().nil(), Ok)
        }
        fn set(&self, _cx: &mut Cx, key: Symbol, value: Value) -> Result<()> {
            self.0.write().unwrap().insert(key, value);
            Ok(())
        }
        fn has(&self, _cx: &mut Cx, key: Symbol) -> Result<bool> {
            Ok(self.0.read().unwrap().contains_key(&key))
        }
        fn del(&self, cx: &mut Cx, key: Symbol) -> Result<Value> {
            self.0
                .write()
                .unwrap()
                .remove(&key)
                .map_or_else(|| cx.factory().nil(), Ok)
        }
        fn keys(&self, _cx: &mut Cx) -> Result<Vec<Symbol>> {
            Ok(self.0.read().unwrap().keys().cloned().collect())
        }
        fn entries(&self, _cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
            Ok(self
                .0
                .read()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect())
        }
        fn len(&self, _cx: &mut Cx) -> Result<usize> {
            Ok(self.0.read().unwrap().len())
        }
        fn clear(&self, _cx: &mut Cx) -> Result<()> {
            self.0.write().unwrap().clear();
            Ok(())
        }
    }
    impl Dir for MemoryDir {
        fn mkdir(&self, _cx: &mut Cx, _name: Symbol) -> Result<Value> {
            Err(Error::Eval("nested dirs unsupported".into()))
        }
        fn opendir(&self, _cx: &mut Cx, _name: Symbol) -> Result<Option<Value>> {
            Ok(None)
        }
        fn rmdir(&self, cx: &mut Cx, _name: Symbol) -> Result<Value> {
            cx.factory().nil()
        }
        fn is_dir(&self, _cx: &mut Cx, _name: Symbol) -> Result<bool> {
            Ok(false)
        }
    }

    fn admission(root: Arc<MemoryDir>) -> JavascriptModuleAdmission {
        JavascriptModuleAdmission {
            root_id: Symbol::new("modules"),
            root,
            read_policy: trusted(),
            requires: vec![module_load_capability()],
            allow: CapabilitySet::new(),
        }
    }

    #[test]
    fn esm_uses_shared_live_lifecycle_supplied_roots_and_cached_failures() {
        let (mut cx, seat) = context();
        seat.grant(&mut cx, read_eval_capability()).unwrap();
        seat.grant(&mut cx, module_load_capability()).unwrap();
        let root = Arc::new(MemoryDir::default());
        root.source(&mut cx, "answer.mjs", "42");
        root.source(&mut cx, "broken.mjs", "(");
        let modules = JavascriptModulePolicy::with_codec(Symbol::qualified("codec", "lisp"));
        let first = modules
            .load(&mut cx, "answer.mjs", admission(root.clone()))
            .unwrap();
        let imported = modules
            .dynamic_import(&mut cx, None, "answer.mjs", admission(root.clone()))
            .unwrap();
        assert_eq!(first.identity(), imported.identity());
        assert_eq!(
            first
                .default_export()
                .get()
                .unwrap()
                .object()
                .display(&mut cx)
                .unwrap(),
            "42"
        );
        assert!(
            modules
                .load(&mut cx, "broken.mjs", admission(root.clone()))
                .is_err()
        );
        root.source(&mut cx, "broken.mjs", "41");
        assert!(
            modules
                .load(&mut cx, "broken.mjs", admission(root))
                .is_err()
        );
        assert_eq!(
            modules
                .receipts()
                .unwrap()
                .iter()
                .map(|r| r.outcome)
                .collect::<Vec<_>>(),
            vec![
                ModuleResolutionOutcome::Linked,
                ModuleResolutionOutcome::CacheHit,
                ModuleResolutionOutcome::Failed,
                ModuleResolutionOutcome::Failed,
            ]
        );
    }

    #[test]
    fn dynamic_source_rejects_ambient_authority() {
        let (mut cx, _seat) = context();
        let dynamic = DynamicJavascript::with_codec(Symbol::qualified("codec", "lisp"));
        let denied = dynamic
            .evaluate(
                &mut cx,
                "42",
                JavascriptDynamicAdmission::new(ReadPolicy {
                    trust: TrustLevel::Untrusted,
                    capabilities: CapabilitySet::new(),
                }),
            )
            .unwrap_err();
        assert!(matches!(
            denied,
            Error::TrustDenied { .. } | Error::CapabilityDenied { .. }
        ));
    }
}
