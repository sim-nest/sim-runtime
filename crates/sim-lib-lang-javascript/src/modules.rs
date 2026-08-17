//! ECMAScript source modules and dynamic source under explicit authority.

use std::sync::Arc;

use sim_kernel::Symbol;
use sim_lib_core::{DynamicSourcePolicy, RequestOrigin};
use sim_lib_namespace::{IdentitySpecifierPolicy, SourceModulePolicy};

/// JavaScript policy over the shared parse/link/evaluate module lifecycle.
///
/// The shared loader canonicalizes supplied-root identities, detects cycles,
/// caches failures, and exposes its default export through a live binding cell.
pub fn javascript_module_policy() -> SourceModulePolicy {
    javascript_module_policy_with_codec(Symbol::qualified("codec", "javascript"))
}

/// Builds JavaScript's module entry for an installed compatible source codec.
pub fn javascript_module_policy_with_codec(codec: Symbol) -> SourceModulePolicy {
    SourceModulePolicy::new(codec, Arc::new(IdentitySpecifierPolicy))
}

/// Builds JavaScript's dynamic-source entry over the shared source policy.
pub fn dynamic_javascript_policy() -> DynamicSourcePolicy {
    dynamic_javascript_policy_with_codec(Symbol::qualified("codec", "javascript"))
}

/// Host-authored root and authority supplied for one ESM resolution.
pub fn dynamic_javascript_policy_with_codec(codec: Symbol) -> DynamicSourcePolicy {
    DynamicSourcePolicy::new(
        codec,
        RequestOrigin::new(Symbol::qualified("javascript", "dynamic-source")),
    )
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::RwLock};

    use sim_codec_lisp::LispCodecLib;
    use sim_kernel::{
        CapabilitySet, ClassId, ClassRef, CodecId, Cx, DefaultFactory, Dir, EagerPolicy, Error,
        Expr, Object, ObjectCompat, ReadPolicy, Result, Table, TrustLevel, Value,
        read_eval_capability,
    };
    use sim_lib_core::SourceAuthority;
    use sim_lib_namespace::{ModuleResolutionOutcome, module_load_capability};
    use sim_shape::{AnyShape, ExactExprShape};

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

    fn authority(requires: Vec<sim_kernel::CapabilityName>) -> SourceAuthority {
        SourceAuthority::new(trusted(), requires, CapabilitySet::new()).unwrap()
    }

    #[test]
    fn esm_uses_shared_live_lifecycle_supplied_roots_and_cached_failures() {
        let (mut cx, seat) = context();
        seat.grant(&mut cx, read_eval_capability()).unwrap();
        seat.grant(&mut cx, module_load_capability()).unwrap();
        let root = Arc::new(MemoryDir::default());
        root.source(&mut cx, "answer.mjs", "42");
        root.source(&mut cx, "broken.mjs", "(");
        let modules = javascript_module_policy_with_codec(Symbol::qualified("codec", "lisp"));
        let first = modules
            .load(
                &mut cx,
                Symbol::new("modules"),
                root.clone(),
                "answer.mjs",
                authority(vec![module_load_capability()]),
            )
            .unwrap();
        let imported = modules
            .dynamic_import(
                &mut cx,
                Symbol::new("modules"),
                root.clone(),
                Some(first.identity().clone()),
                "./answer.mjs",
                authority(vec![module_load_capability()]),
            )
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
                .load(
                    &mut cx,
                    Symbol::new("modules"),
                    root.clone(),
                    "broken.mjs",
                    authority(Vec::new())
                )
                .is_err()
        );
        root.source(&mut cx, "broken.mjs", "41");
        assert!(
            modules
                .load(
                    &mut cx,
                    Symbol::new("modules"),
                    root,
                    "broken.mjs",
                    authority(Vec::new())
                )
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
                ModuleResolutionOutcome::DecodeFailed,
                ModuleResolutionOutcome::DecodeFailed,
            ]
        );
    }

    #[test]
    fn dynamic_source_rejects_ambient_authority() {
        let (mut cx, seat) = context();
        let dynamic = dynamic_javascript_policy_with_codec(Symbol::qualified("codec", "lisp"));
        let denied = SourceAuthority::new(
            ReadPolicy {
                trust: TrustLevel::Untrusted,
                capabilities: CapabilitySet::new(),
            },
            Vec::new(),
            CapabilitySet::new(),
        )
        .unwrap_err();
        assert!(matches!(
            denied,
            Error::TrustDenied { .. } | Error::CapabilityDenied { .. }
        ));

        seat.grant(&mut cx, read_eval_capability()).unwrap();
        let denied = dynamic
            .evaluate_text(
                &mut cx,
                "42",
                authority(Vec::new()),
                Arc::new(ExactExprShape::new(Expr::String("not-42".into()))),
            )
            .unwrap_err();
        assert!(matches!(denied, Error::WrongShape { .. }));
        let value = dynamic
            .evaluate_text(&mut cx, "42", authority(Vec::new()), Arc::new(AnyShape))
            .unwrap();
        assert_eq!(value.object().display(&mut cx).unwrap(), "42");
    }
}
