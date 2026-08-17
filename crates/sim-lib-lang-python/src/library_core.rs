//! Capability-bounded matching, source modules, and dynamic evaluation.

use std::sync::Arc;

use sim_kernel::{Cx, Expr, Result, Shape, ShapeBindings, Symbol};
use sim_lib_core::{DynamicSourcePolicy, RequestOrigin};
use sim_lib_namespace::{IdentitySpecifierPolicy, SourceModulePolicy};

use crate::python_core_matrix_row;

/// Whether one public Python surface is implemented by the checked profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PythonSurfaceState {
    /// The checked matrix exercises this member and the profile exposes it.
    Present,
    /// The profile deliberately has no implementation for this member.
    Absent,
}

/// One explicit builtin or curated source-library member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonSurface {
    /// Python-visible qualified name.
    pub name: &'static str,
    /// Checked presence rather than an implicit host fallback.
    pub state: PythonSurfaceState,
    /// Matrix cases proving a present member, or the stable exclusion reason.
    pub evidence: Vec<String>,
}

/// Generated builtin and source-library coverage for the checked matrix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PythonLibraryManifest {
    /// Matrix-derived builtin coverage followed by explicit absences.
    pub builtins: Vec<PythonSurface>,
    /// Matrix-derived curated module coverage followed by explicit absences.
    pub modules: Vec<PythonSurface>,
}

const BUILTIN_RULES: &[(&str, &[&str])] = &[
    ("range", &["scalar-flow"]),
    ("property", &["objects-c3-descriptors-super"]),
    ("super", &["objects-c3-descriptors-super"]),
    ("next", &["generator-send-close"]),
    ("eval", &["authorized-dynamic-eval"]),
    ("exec", &["authorized-dynamic-exec"]),
];
const ABSENT_BUILTINS: &[(&str, &str)] = &[
    ("compile", "no compiler or bytecode surface"),
    ("open", "storage is supplied through Table/Dir roots"),
    ("__import__", "imports use the shared module lifecycle"),
    ("breakpoint", "no ambient debugger or host process"),
    ("input", "no ambient terminal"),
];
const MODULE_RULES: &[(&str, &[&str])] = &[
    ("sim.safe_eval", &["authorized-dynamic-eval"]),
    ("sim.safe_exec", &["authorized-dynamic-exec"]),
];
const ABSENT_MODULES: &[(&str, &str)] = &[
    ("os", "no ambient host access"),
    ("sys", "no foreign runtime or process state"),
    (
        "subprocess",
        "host exec is a separate capability-gated library",
    ),
    (
        "socket",
        "network access is a separate capability-gated library",
    ),
    ("pathlib", "paths are supplied Table/Dir identities"),
];

/// Generate the public library manifest directly from the checked Python row.
///
/// A rule whose matrix case is missing becomes absent. This makes matrix drift
/// reduce claims instead of silently retaining an authored promise.
pub fn python_library_manifest() -> PythonLibraryManifest {
    let row = python_core_matrix_row();
    let case_names = row
        .cases
        .iter()
        .filter(|case| case.symbol.namespace.as_deref() == Some("test/python-core"))
        .map(|case| case.symbol.name.to_string())
        .collect::<Vec<_>>();
    let derive = |rules: &[(&'static str, &'static [&'static str])],
                  absent: &[(&'static str, &'static str)]| {
        rules
            .iter()
            .map(|(name, needs)| {
                let evidence = needs
                    .iter()
                    .filter(|needed| case_names.iter().any(|case| case == **needed))
                    .map(|needed| (*needed).to_owned())
                    .collect::<Vec<_>>();
                PythonSurface {
                    name,
                    state: if evidence.len() == needs.len() {
                        PythonSurfaceState::Present
                    } else {
                        PythonSurfaceState::Absent
                    },
                    evidence: if evidence.len() == needs.len() {
                        evidence
                    } else {
                        vec![format!("missing checked matrix case: {}", needs.join(", "))]
                    },
                }
            })
            .chain(absent.iter().map(|(name, reason)| PythonSurface {
                name,
                state: PythonSurfaceState::Absent,
                evidence: vec![(*reason).to_owned()],
            }))
            .collect()
    };
    PythonLibraryManifest {
        builtins: derive(BUILTIN_RULES, ABSENT_BUILTINS),
        modules: derive(MODULE_RULES, ABSENT_MODULES),
    }
}

/// One ordered Python structural-match case composed from a canonical Shape.
type MatchGuard<'a> = dyn FnMut(&mut Cx, &ShapeBindings) -> Result<bool> + 'a;

/// One ordered Python structural-match case composed from a canonical Shape.
pub struct MatchCase<'a> {
    /// Shape/pattern that performs structural checking and captures.
    pub pattern: Arc<dyn Shape>,
    /// Optional guard evaluated only after this pattern accepts.
    pub guard: Option<&'a mut MatchGuard<'a>>,
}

/// Result of ordered structural matching.
pub enum MatchOutcome {
    /// First pattern whose guard accepted, including its isolated bindings.
    Matched {
        /// Zero-based declaration-order case index.
        index: usize,
        /// Captures produced only by the accepted case.
        bindings: ShapeBindings,
    },
    /// No faithfully supported case accepted.
    NoMatch,
}

/// Match an expression in declaration order using Shape captures and guards.
///
/// Captures from rejected patterns and false guards never escape into later
/// cases, matching Python's case-local binding policy.
pub fn match_expr(
    cx: &mut Cx,
    subject: &Expr,
    cases: &mut [MatchCase<'_>],
) -> Result<MatchOutcome> {
    for (index, case) in cases.iter_mut().enumerate() {
        let matched = case.pattern.check_expr(cx, subject)?;
        if !matched.accepted {
            continue;
        }
        if let Some(guard) = case.guard.as_mut()
            && !guard(cx, &matched.captures)?
        {
            continue;
        }
        return Ok(MatchOutcome::Matched {
            index,
            bindings: matched.captures,
        });
    }
    Ok(MatchOutcome::NoMatch)
}

/// Build Python's module entry over the shared source-module policy.
pub fn python_module_policy() -> SourceModulePolicy {
    python_module_policy_with_codec(Symbol::qualified("codec", "python"))
}

/// Build Python's module entry for an installed compatible source codec.
pub fn python_module_policy_with_codec(codec: Symbol) -> SourceModulePolicy {
    SourceModulePolicy::new(codec, Arc::new(IdentitySpecifierPolicy))
}

/// Build one Python dynamic-source entry over the shared source policy.
pub fn dynamic_python_policy(operation: &str) -> DynamicSourcePolicy {
    dynamic_python_policy_with_codec(operation, Symbol::qualified("codec", "python"))
}

/// Build one Python dynamic-source entry for an installed compatible codec.
pub fn dynamic_python_policy_with_codec(operation: &str, codec: Symbol) -> DynamicSourcePolicy {
    DynamicSourcePolicy::new(
        codec,
        RequestOrigin::new(Symbol::qualified("python", operation)),
    )
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, collections::BTreeMap, sync::RwLock};

    use sim_codec_lisp::LispCodecLib;
    use sim_kernel::{
        CapabilityName, CapabilitySet, ClassId, ClassRef, CodecId, DefaultFactory, Dir,
        EagerPolicy, Error, Object, ObjectCompat, ReadPolicy, Table, TrustLevel, Value,
        read_eval_capability,
    };
    use sim_lib_core::SourceAuthority;
    use sim_lib_namespace::{ModuleResolutionOutcome, module_load_capability};
    use sim_shape::{AnyShape, CaptureShape, ExactExprShape, ListShape};

    use super::*;

    fn context() -> (Cx, sim_kernel::GrantSeat) {
        let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
        cx.load_lib(&LispCodecLib::new(CodecId(93)).unwrap())
            .unwrap();
        (cx, seat)
    }

    fn trusted() -> ReadPolicy {
        ReadPolicy {
            trust: TrustLevel::TrustedSource,
            capabilities: CapabilitySet::new().grant(read_eval_capability()),
        }
    }

    fn authority(requires: Vec<CapabilityName>) -> SourceAuthority {
        SourceAuthority::new(trusted(), requires, CapabilitySet::new()).unwrap()
    }

    #[test]
    fn structural_match_preserves_order_guard_and_case_local_bindings() {
        let (mut cx, _seat) = context();
        let capture = || {
            Arc::new(CaptureShape::new(Symbol::new("item"), Arc::new(AnyShape))) as Arc<dyn Shape>
        };
        let tuple = || {
            Arc::new(ListShape::new(vec![
                Arc::new(ExactExprShape::new(Expr::String("left".to_owned()))),
                capture(),
            ])) as Arc<dyn Shape>
        };
        let rejected_guard_calls = Cell::new(0);
        let mut first_guard = |_cx: &mut Cx, bindings: &ShapeBindings| {
            rejected_guard_calls.set(rejected_guard_calls.get() + 1);
            Ok(bindings.exprs().len() == 99)
        };
        let mut second_guard =
            |_cx: &mut Cx, bindings: &ShapeBindings| Ok(bindings.exprs().len() == 1);
        let mut cases = [
            MatchCase {
                pattern: tuple(),
                guard: Some(&mut first_guard),
            },
            MatchCase {
                pattern: tuple(),
                guard: Some(&mut second_guard),
            },
        ];
        let outcome = match_expr(
            &mut cx,
            &Expr::List(vec![
                Expr::String("left".to_owned()),
                Expr::String("right".to_owned()),
            ]),
            &mut cases,
        )
        .unwrap();
        assert_eq!(rejected_guard_calls.get(), 1);
        let MatchOutcome::Matched { index, bindings } = outcome else {
            panic!("expected match")
        };
        assert_eq!(index, 1);
        assert_eq!(
            bindings.exprs(),
            &[(Symbol::new("item"), Expr::String("right".to_owned()))]
        );
        assert!(matches!(
            match_expr(&mut cx, &Expr::String("gap".to_owned()), &mut cases).unwrap(),
            MatchOutcome::NoMatch
        ));
    }

    #[test]
    fn generated_manifest_tracks_matrix_and_makes_absence_explicit() {
        let manifest = python_library_manifest();
        assert!(
            manifest
                .builtins
                .iter()
                .find(|item| item.name == "eval")
                .is_some_and(|item| item.state == PythonSurfaceState::Present
                    && item.evidence == ["authorized-dynamic-eval"])
        );
        for absent in ["compile", "open", "__import__", "breakpoint", "input"] {
            assert!(
                manifest
                    .builtins
                    .iter()
                    .find(|item| item.name == absent)
                    .is_some_and(|item| item.state == PythonSurfaceState::Absent
                        && !item.evidence.is_empty())
            );
        }
        for absent in ["os", "sys", "subprocess", "socket", "pathlib"] {
            assert!(
                manifest
                    .modules
                    .iter()
                    .find(|item| item.name == absent)
                    .is_some_and(|item| item.state == PythonSurfaceState::Absent
                        && !item.evidence.is_empty())
            );
        }
    }

    #[test]
    fn dynamic_eval_and_exec_require_authority_and_diminish_it() {
        let (mut cx, seat) = context();
        let eval = dynamic_python_policy_with_codec("eval", Symbol::qualified("codec", "lisp"));
        let exec = dynamic_python_policy_with_codec("exec", Symbol::qualified("codec", "lisp"));
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
        let missing = CapabilityName::new("python.dynamic.required");
        let denied = exec
            .evaluate_text(
                &mut cx,
                "42",
                authority(vec![missing.clone()]),
                Arc::new(AnyShape),
            )
            .unwrap_err();
        assert!(matches!(denied, Error::CapabilityDenied { .. }));
        seat.grant(&mut cx, missing).unwrap();
        let value = eval
            .evaluate_text(&mut cx, "42", authority(Vec::new()), Arc::new(AnyShape))
            .unwrap();
        assert_eq!(value.object().display(&mut cx).unwrap(), "42");
        let value = exec
            .evaluate_text(&mut cx, "42", authority(Vec::new()), Arc::new(AnyShape))
            .unwrap();
        assert_eq!(value.object().display(&mut cx).unwrap(), "42");

        let denied = eval
            .evaluate_text(
                &mut cx,
                "42",
                authority(Vec::new()),
                Arc::new(ExactExprShape::new(Expr::String("not-42".into()))),
            )
            .unwrap_err();
        assert!(matches!(denied, Error::WrongShape { .. }));
    }

    #[derive(Default)]
    struct MemoryDir {
        files: RwLock<BTreeMap<Symbol, Value>>,
    }

    impl MemoryDir {
        fn source(&self, cx: &mut Cx, name: &str, source: &str) {
            self.files.write().unwrap().insert(
                Symbol::new(name),
                cx.factory().string(source.to_owned()).unwrap(),
            );
        }
    }
    impl Object for MemoryDir {
        fn display(&self, _cx: &mut Cx) -> Result<String> {
            Ok("python-memory-root".to_owned())
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    impl ObjectCompat for MemoryDir {
        fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
            cx.factory()
                .class_stub(ClassId(0), Symbol::qualified("test", "PythonRoot"))
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
            Symbol::qualified("test", "python-root")
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
                .map(|(key, value)| (key.clone(), value.clone()))
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
        fn mkdir(&self, _cx: &mut Cx, _name: Symbol) -> Result<Value> {
            Err(Error::Eval("nested fixture dirs unsupported".to_owned()))
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

    #[test]
    fn modules_use_supplied_dir_shared_lifecycle_and_cache_failures() {
        let (mut cx, seat) = context();
        seat.grant(&mut cx, read_eval_capability()).unwrap();
        let root = Arc::new(MemoryDir::default());
        root.source(&mut cx, "answer.py", "42");
        let modules = python_module_policy_with_codec(Symbol::qualified("codec", "lisp"));
        let load = |modules: &SourceModulePolicy,
                    cx: &mut Cx,
                    root: Arc<MemoryDir>,
                    specifier: &str,
                    requires| {
            modules.load(
                cx,
                Symbol::new("supplied"),
                root,
                specifier,
                authority(requires),
            )
        };
        let denied = modules
            .load(
                &mut cx,
                Symbol::new("supplied"),
                root.clone(),
                "answer.py",
                authority(vec![module_load_capability()]),
            )
            .unwrap_err();
        assert!(matches!(denied, Error::CapabilityDenied { .. }));
        seat.grant(&mut cx, module_load_capability()).unwrap();
        let loaded = load(
            &modules,
            &mut cx,
            root.clone(),
            "answer.py",
            vec![module_load_capability()],
        )
        .unwrap();
        assert_eq!(
            loaded
                .default_export()
                .get()
                .unwrap()
                .object()
                .display(&mut cx)
                .unwrap(),
            "42"
        );
        root.source(&mut cx, "broken.py", "(");
        assert!(load(&modules, &mut cx, root.clone(), "broken.py", vec![]).is_err());
        root.source(&mut cx, "broken.py", "41");
        assert!(load(&modules, &mut cx, root, "broken.py", vec![]).is_err());
        assert_eq!(
            modules
                .receipts()
                .unwrap()
                .iter()
                .map(|receipt| receipt.outcome)
                .collect::<Vec<_>>(),
            vec![
                ModuleResolutionOutcome::Linked,
                ModuleResolutionOutcome::DecodeFailed,
                ModuleResolutionOutcome::DecodeFailed
            ]
        );
    }
}
