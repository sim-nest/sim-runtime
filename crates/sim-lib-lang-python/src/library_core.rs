//! Capability-bounded matching, source modules, and dynamic evaluation.

use std::sync::Arc;

use sim_kernel::{
    CapabilityName, CapabilitySet, Cx, Dir, Expr, ReadPolicy, Result, Shape, ShapeBindings, Symbol,
    Value,
};
use sim_lib_core::{ReadEvalBroker, ReadEvalRequest, ReadEvalSource, RequestOrigin};
use sim_lib_namespace::{ModuleInstance, ModuleLoader, ModuleRequest};
use sim_shape::AnyShape;

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

/// Python policy wrapper around the canonical source-module lifecycle.
pub struct PythonModulePolicy {
    loader: ModuleLoader,
    codec: Symbol,
}

impl Default for PythonModulePolicy {
    fn default() -> Self {
        Self::with_codec(Symbol::qualified("codec", "python"))
    }
}

impl PythonModulePolicy {
    /// Build a module policy for an installed compatible source codec.
    pub fn with_codec(codec: Symbol) -> Self {
        Self {
            loader: ModuleLoader::new(),
            codec,
        }
    }

    /// Load a `.py` source module from the only supplied directory root.
    pub fn load(
        &self,
        cx: &mut Cx,
        specifier: impl Into<String>,
        admission: PythonModuleAdmission,
    ) -> Result<ModuleInstance> {
        self.loader.load(
            cx,
            ModuleRequest {
                root_id: admission.root_id,
                root: admission.root,
                importer: None,
                specifier: specifier.into(),
                codec: self.codec.clone(),
                read_policy: admission.read_policy,
                requires: admission.requires,
                allow: admission.allow,
            },
        )
    }

    /// Inspect canonical lifecycle receipts, including cycles and cached failures.
    pub fn receipts(&self) -> Result<Vec<sim_lib_namespace::ModuleResolutionReceipt>> {
        self.loader.receipts()
    }
}

/// Host-authored storage and authority envelope for one Python module load.
pub struct PythonModuleAdmission {
    /// Stable caller-assigned identity for the supplied root.
    pub root_id: Symbol,
    /// The only directory visible to module resolution.
    pub root: Arc<dyn Dir>,
    /// Trusted policy used by diminished read-eval.
    pub read_policy: ReadPolicy,
    /// Powers the importing caller must already hold.
    pub requires: Vec<CapabilityName>,
    /// Diminished powers visible while decoding and evaluating.
    pub allow: CapabilitySet,
}

/// Capability-gated Python `eval` and `exec` with no ambient authority.
pub struct DynamicPython {
    broker: ReadEvalBroker,
    codec: Symbol,
}

impl Default for DynamicPython {
    fn default() -> Self {
        Self::with_codec(Symbol::qualified("codec", "python"))
    }
}

impl DynamicPython {
    /// Build the dynamic surface for an installed compatible source codec.
    pub fn with_codec(codec: Symbol) -> Self {
        Self {
            broker: ReadEvalBroker::new(),
            codec,
        }
    }

    /// Evaluate text through the installed Python codec under diminished powers.
    pub fn eval(
        &self,
        cx: &mut Cx,
        source: impl Into<String>,
        admission: DynamicAdmission,
    ) -> Result<Value> {
        self.admit(cx, "eval", source.into(), admission)
    }

    /// Execute text through the same installed codec and diminished read-eval gate.
    pub fn exec(
        &self,
        cx: &mut Cx,
        source: impl Into<String>,
        admission: DynamicAdmission,
    ) -> Result<Value> {
        self.admit(cx, "exec", source.into(), admission)
    }

    fn admit(
        &self,
        cx: &mut Cx,
        operation: &str,
        source: String,
        admission: DynamicAdmission,
    ) -> Result<Value> {
        self.broker.admit(
            cx,
            ReadEvalRequest {
                origin: RequestOrigin::new(Symbol::qualified("python", operation)),
                codec: self.codec.clone(),
                source: ReadEvalSource::Text(source),
                read_policy: admission.read_policy,
                requires: admission.requires,
                allow: admission.allow,
                expected_shape: admission.expected_shape,
            },
        )
    }
}

/// Host-authored authority envelope for dynamic Python source.
pub struct DynamicAdmission {
    /// Trusted read policy; source text cannot create this value.
    pub read_policy: ReadPolicy,
    /// Powers the caller must hold.
    pub requires: Vec<CapabilityName>,
    /// Diminished powers visible while decoding and evaluating.
    pub allow: CapabilitySet,
    /// Shape required of the resulting value.
    pub expected_shape: Arc<dyn Shape>,
}

impl DynamicAdmission {
    /// Build an admission envelope requiring only the canonical read-eval power.
    pub fn new(read_policy: ReadPolicy, allow: CapabilitySet) -> Self {
        Self {
            read_policy,
            requires: Vec::new(),
            allow,
            expected_shape: Arc::new(AnyShape),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, collections::BTreeMap, sync::RwLock};

    use sim_codec_lisp::LispCodecLib;
    use sim_kernel::{
        ClassId, ClassRef, CodecId, DefaultFactory, EagerPolicy, Error, Object, ObjectCompat,
        Table, TrustLevel, read_eval_capability,
    };
    use sim_lib_namespace::{ModuleResolutionOutcome, module_load_capability};
    use sim_shape::{CaptureShape, ExactExprShape, ListShape};

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
        let dynamic = DynamicPython::with_codec(Symbol::qualified("codec", "lisp"));
        let denied = dynamic
            .eval(
                &mut cx,
                "42",
                DynamicAdmission::new(
                    ReadPolicy {
                        trust: TrustLevel::Untrusted,
                        capabilities: CapabilitySet::new(),
                    },
                    CapabilitySet::new(),
                ),
            )
            .unwrap_err();
        assert!(matches!(
            denied,
            Error::TrustDenied { .. } | Error::CapabilityDenied { .. }
        ));

        seat.grant(&mut cx, read_eval_capability()).unwrap();
        let missing = CapabilityName::new("python.dynamic.required");
        let denied = dynamic
            .exec(
                &mut cx,
                "42",
                DynamicAdmission {
                    read_policy: trusted(),
                    requires: vec![missing.clone()],
                    allow: CapabilitySet::new(),
                    expected_shape: Arc::new(AnyShape),
                },
            )
            .unwrap_err();
        assert!(matches!(denied, Error::CapabilityDenied { .. }));
        seat.grant(&mut cx, missing).unwrap();
        let value = dynamic
            .eval(
                &mut cx,
                "42",
                DynamicAdmission::new(trusted(), CapabilitySet::new()),
            )
            .unwrap();
        assert_eq!(value.object().display(&mut cx).unwrap(), "42");
        let value = dynamic
            .exec(
                &mut cx,
                "42",
                DynamicAdmission::new(trusted(), CapabilitySet::new()),
            )
            .unwrap();
        assert_eq!(value.object().display(&mut cx).unwrap(), "42");

        let denied = dynamic
            .eval(
                &mut cx,
                "42",
                DynamicAdmission {
                    read_policy: trusted(),
                    requires: Vec::new(),
                    allow: CapabilitySet::new(),
                    expected_shape: Arc::new(ExactExprShape::new(Expr::String("not-42".into()))),
                },
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
        let modules = PythonModulePolicy::with_codec(Symbol::qualified("codec", "lisp"));
        let admission = |root: Arc<MemoryDir>, requires| PythonModuleAdmission {
            root_id: Symbol::new("supplied"),
            root,
            read_policy: trusted(),
            requires,
            allow: CapabilitySet::new(),
        };
        let denied = modules
            .load(
                &mut cx,
                "answer.py",
                admission(root.clone(), vec![module_load_capability()]),
            )
            .unwrap_err();
        assert!(matches!(denied, Error::CapabilityDenied { .. }));
        seat.grant(&mut cx, module_load_capability()).unwrap();
        let loaded = modules
            .load(
                &mut cx,
                "answer.py",
                admission(root.clone(), vec![module_load_capability()]),
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
        assert!(
            modules
                .load(&mut cx, "broken.py", admission(root.clone(), vec![]),)
                .is_err()
        );
        root.source(&mut cx, "broken.py", "41");
        assert!(
            modules
                .load(&mut cx, "broken.py", admission(root, vec![]),)
                .is_err()
        );
        assert_eq!(
            modules
                .receipts()
                .unwrap()
                .iter()
                .map(|receipt| receipt.outcome)
                .collect::<Vec<_>>(),
            vec![
                ModuleResolutionOutcome::Linked,
                ModuleResolutionOutcome::Failed,
                ModuleResolutionOutcome::Failed
            ]
        );
    }
}
