//! Ordinary SIM-facing JVM callables, bounded browsing, and profile evidence.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use sim_codec_classfile::{ByteReader, CodeAttribute, Constant, decode_instructions};
use sim_incremental_core::ValueFingerprint;
use sim_kernel::{
    AbiVersion, Args, Callable, CapabilityName, ClassRef, Cx, Error, Export, Expr, Lib,
    LibManifest, LibTarget, Linker, Object, ObjectCompat, Result, ShapeRef, Symbol, Value, Version,
};
use sim_lib_standard_core::{FidelityBadge, LanguageProfile, OrganUse};
use sim_shape::AnyShape;

use crate::{
    ClassDefinition, ClassLoader, ClassVerificationProof, LineageBudget, VerificationState,
};

include!("surface/entry_policy.rs");

/// Typed completion lanes for an integer JVM invocation.
#[derive(Clone, Debug)]
pub enum JvmInvocationError {
    /// The call contradicted the selected member or supported descriptor before effects.
    Admission(String),
    /// Admitted execution exhausted bounded machine storage.
    Resource(String),
    /// Guest execution completed exceptionally with a Java-owned condition.
    JavaThrowable(Box<crate::JavaThrowable>),
}

/// One caller-selected, bounded JVM invocation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmExecutionRequest {
    /// Complete classfile bytes already selected by the caller's source authority.
    pub classfile: Vec<u8>,
    /// Exact binary class name claimed by the classfile.
    pub class: String,
    /// Exact static member name.
    pub member: String,
    /// Exact JVM descriptor.
    pub descriptor: String,
    /// Integer arguments in descriptor order.
    pub arguments: Vec<i32>,
}

/// Disjoint completion lanes for the public caller-selected JVM route.
#[derive(Debug)]
pub enum JvmExecutionOutcome {
    /// Java bytecode returned an integer value.
    Value(i32),
    /// Java bytecode completed abruptly with a Java throwable.
    Throwable(Box<crate::JavaThrowable>),
    /// Admission refused the request before successful execution.
    Refusal(String),
}

impl std::fmt::Display for JvmInvocationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Admission(detail) => {
                write!(formatter, "JVM invocation admission refused: {detail}")
            }
            Self::Resource(detail) => {
                write!(formatter, "JVM invocation resource exhausted: {detail}")
            }
            Self::JavaThrowable(throwable) => {
                write!(
                    formatter,
                    "JVM invocation raised {:?}",
                    throwable.condition()
                )
            }
        }
    }
}

impl std::error::Error for JvmInvocationError {}

impl From<JvmInvocationError> for Error {
    fn from(error: JvmInvocationError) -> Self {
        Self::Eval(error.to_string())
    }
}

/// The profile's declared absences, ordered before all positive fidelity claims.
pub const JVM_DECLARED_ABSENCES: [&str; 3] =
    ["no-verification", "no-class-library", "no-lambda-linkage"];

/// Capability required for static or instance JVM invocation.
pub fn jvm_invoke_capability() -> CapabilityName {
    CapabilityName::new("jvm.invoke")
}

/// Capability required for class, method, code, or heap browsing.
pub fn jvm_browse_capability() -> CapabilityName {
    CapabilityName::new("jvm.browse")
}

/// Registered JVM profile with its absences declared before any positive badge.
pub fn jvm_language_profile() -> LanguageProfile {
    let profile = Symbol::qualified("lang", "jvm/v1");
    let mut value = LanguageProfile::new(profile.clone())
        .with_reader(Symbol::qualified("codec", "classfile"))
        .with_lowering(Symbol::qualified("jvm", "classfile-lowering"))
        .with_eval_policy(Symbol::qualified("jvm", "bounded-eval"))
        .with_organ(OrganUse::new(Symbol::qualified("organ", "machine")))
        .requiring(crate::class_load_capability())
        .requiring(jvm_invoke_capability());
    for absence in JVM_DECLARED_ABSENCES {
        value = value.with_unsupported_form(Symbol::qualified("jvm", absence));
    }
    value.with_fidelity_badge(FidelityBadge::new(
        sim_kernel::Ref::Symbol(profile),
        Symbol::qualified("jvm", "bounded-classfile-execution"),
        1,
        sim_kernel::Ref::Symbol(Symbol::qualified("recipe", "jvm-authorized-static-call")),
    ))
}

/// Deterministic bounded browse projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmBrowse {
    /// Loaded binary class name.
    pub class: String,
    /// Method signatures, in classfile order and truncated to the requested bound.
    pub methods: Vec<String>,
    /// Bytecode lengths paired with the corresponding methods.
    pub code_bytes: Vec<usize>,
}

/// Shared state behind the loadable JVM callables.
pub struct JvmSurface {
    loader: ClassLoader,
    heap: Mutex<crate::JvmHeap>,
    frames: crate::JvmFramePool,
    last_receipts: Mutex<Option<(crate::JvmPreparationReceipt, crate::JvmDriveReceipt)>>,
    prepared: Mutex<BTreeMap<String, Arc<PreparedSurfaceMethod>>>,
    decode_count: AtomicUsize,
    _lineage_budget: LineageBudget,
}

impl JvmSurface {
    /// Creates an isolated surface with a hard classfile byte allowance.
    pub fn new(max_classfile_bytes: usize) -> Self {
        Self::with_lineage_budget(
            max_classfile_bytes,
            LineageBudget {
                nodes: 256,
                work: 4_096,
            },
        )
    }

    /// Creates an isolated surface with explicit classfile and lineage bounds.
    pub fn with_lineage_budget(max_classfile_bytes: usize, lineage_budget: LineageBudget) -> Self {
        Self {
            loader: ClassLoader::new(max_classfile_bytes),
            heap: Mutex::new(crate::JvmHeap::surface_default()),
            frames: crate::JvmFramePool::new(crate::JvmFramePoolPolicy {
                frames: 64,
                slots: 4_096,
                operands: 4_096,
            }),
            last_receipts: Mutex::new(None),
            prepared: Mutex::new(BTreeMap::new()),
            decode_count: AtomicUsize::new(0),
            _lineage_budget: lineage_budget,
        }
    }

    /// Defines caller-supplied bytes without consulting an ambient loader or transport.
    pub fn define(&self, cx: &mut Cx, name: &str, bytes: Vec<u8>) -> Result<Arc<ClassDefinition>> {
        self.loader.define_bytes(cx, name, bytes)
    }

    /// Loads and invokes one caller-selected classfile without an ambient classpath.
    pub fn execute_i32(&self, cx: &mut Cx, request: JvmExecutionRequest) -> JvmExecutionOutcome {
        if let Err(error) = self.define(cx, &request.class, request.classfile) {
            return JvmExecutionOutcome::Refusal(error.to_string());
        }
        match self.invoke_static_i32(
            cx,
            &request.class,
            &request.member,
            &request.descriptor,
            &request.arguments,
        ) {
            Ok(value) => JvmExecutionOutcome::Value(value),
            Err(JvmInvocationError::JavaThrowable(throwable)) => {
                JvmExecutionOutcome::Throwable(throwable)
            }
            Err(error) => JvmExecutionOutcome::Refusal(error.to_string()),
        }
    }

    /// Returns the number of execution frames currently held by live calls.
    pub fn live_frame_leases(&self) -> usize {
        self.frames.live_leases()
    }

    /// Returns the last completed preparation and execution evidence.
    pub fn last_drive_receipts(
        &self,
    ) -> Option<(crate::JvmPreparationReceipt, crate::JvmDriveReceipt)> {
        self.last_receipts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Returns the number of classfile method bodies decoded by this surface.
    pub fn decode_count(&self) -> usize {
        self.decode_count.load(Ordering::Relaxed)
    }

    /// Invokes a bounded integer-only static method through exact JVM selection.
    pub fn invoke_static_i32(
        &self,
        cx: &mut Cx,
        class: &str,
        name: &str,
        descriptor: &str,
        args: &[i32],
    ) -> std::result::Result<i32, JvmInvocationError> {
        self.invoke_static_i32_with_policy(
            cx,
            class,
            name,
            descriptor,
            args,
            JvmEntryPolicy::StaticChecked,
        )
    }

    /// Invokes a static method under an explicit static-checked or verified entry policy.
    pub fn invoke_static_i32_with_policy(
        &self,
        cx: &mut Cx,
        class: &str,
        name: &str,
        descriptor: &str,
        args: &[i32],
        policy: JvmEntryPolicy<'_>,
    ) -> std::result::Result<i32, JvmInvocationError> {
        cx.require(&jvm_invoke_capability())?;
        let definition = self
            .loader
            .loaded(class)?
            .ok_or_else(|| Error::Eval(format!("JVM class {class} is not defined")))?;
        let member = definition
            .metadata()
            .select_method(name, descriptor)
            .ok_or_else(|| Error::Eval(format!("missing JVM method {class}.{name}{descriptor}")))?;
        if !member.is_static() {
            return Err(invocation_admission("selected member is not static"));
        }
        let descriptor = admit_i32_descriptor(descriptor, args.len())?;
        let (value, preparation, execution) = execute_prepared_i32(
            &self.loader,
            &self.heap,
            &self.frames,
            &self.prepared,
            &self.decode_count,
            &definition,
            name,
            &descriptor,
            args,
            policy,
        )?;
        *self
            .last_receipts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some((preparation, execution));
        Ok(value)
    }

    /// Invokes a bounded integer-only instance method after JVM virtual selection.
    pub fn invoke_instance_i32(
        &self,
        cx: &mut Cx,
        declaring: &str,
        receiver: &str,
        name: &str,
        descriptor: &str,
        args: &[i32],
    ) -> std::result::Result<i32, JvmInvocationError> {
        cx.require(&jvm_invoke_capability())?;
        let _declared = self
            .loader
            .loaded(declaring)?
            .ok_or_else(|| Error::Eval(format!("JVM class {declaring} is not defined")))?;
        let _ = (
            receiver,
            name,
            admit_i32_descriptor(descriptor, args.len())?,
        );
        Err(invocation_admission(
            "instance invocation requires a surface-owned receiver object",
        ))
    }

    /// Browses classes, methods, code sizes, and the opaque heap count under one bound.
    pub fn browse(&self, cx: &mut Cx, limit: usize) -> Result<Vec<JvmBrowse>> {
        cx.require(&jvm_browse_capability())?;
        self.loader
            .browse_classes(limit)?
            .into_iter()
            .map(|class| {
                let methods = class
                    .metadata()
                    .members()
                    .iter()
                    .filter(|m| matches!(m.kind(), crate::JavaMemberKind::Method))
                    .take(limit)
                    .map(|m| format!("{}{}", m.name(), m.descriptor()))
                    .collect::<Vec<_>>();
                let code_bytes = method_code_lengths(&class)
                    .into_iter()
                    .take(limit)
                    .collect();
                Ok(JvmBrowse {
                    class: class.id().binary_name().into(),
                    methods,
                    code_bytes,
                })
            })
            .collect()
    }
}

struct PreparedSurfaceMethod {
    code: sim_lib_machine::LocatedCode<crate::PreparedJvmPolicy>,
    machine: sim_lib_machine::MachinePermit,
    limits: sim_lib_machine::AdmissionLimits,
    max_locals: usize,
    max_stack: usize,
}

fn method_code_lengths(class: &ClassDefinition) -> Vec<usize> {
    class
        .shell()
        .methods
        .iter()
        .filter_map(|method| {
            code_attribute(class, method)
                .ok()
                .flatten()
                .map(|code| code.code.len())
        })
        .collect()
}

fn code_attribute(
    class: &ClassDefinition,
    method: &sim_codec_classfile::MethodShell,
) -> Result<Option<CodeAttribute>> {
    for attribute in &method.attributes {
        let Some(Constant::Utf8(name)) = class
            .shell()
            .constant_pool
            .slots()
            .get(usize::from(attribute.name_index))
            .and_then(|slot| match slot {
                sim_codec_classfile::ConstantSlot::Entry(value) => Some(value),
                _ => None,
            })
        else {
            continue;
        };
        if name.as_code_units() == ['C' as u16, 'o' as u16, 'd' as u16, 'e' as u16] {
            return CodeAttribute::decode(&mut ByteReader::new(
                &attribute.bytes,
                attribute.bytes.len().max(1),
            ))
            .map(Some)
            .map_err(|e| Error::Eval(e.to_string()));
        }
    }
    Ok(None)
}

include!("surface/execute_entry.rs");

struct AdmittedI32Descriptor {
    text: String,
}

fn admit_i32_descriptor(
    descriptor: &str,
    supplied: usize,
) -> std::result::Result<AdmittedI32Descriptor, JvmInvocationError> {
    let (parameters, result) = crate::linker::split_method_descriptor(descriptor)
        .map_err(|_| invocation_admission(format!("malformed method descriptor {descriptor}")))?;
    if parameters.iter().any(|parameter| parameter != "I") {
        return Err(invocation_admission(format!(
            "descriptor {descriptor} contains a non-int parameter"
        )));
    }
    if result != "I" {
        return Err(invocation_admission(format!(
            "descriptor {descriptor} does not return int"
        )));
    }
    if supplied != parameters.len() {
        return Err(invocation_admission(format!(
            "descriptor {descriptor} requires {} arguments, received {supplied}",
            parameters.len()
        )));
    }
    Ok(AdmittedI32Descriptor {
        text: descriptor.into(),
    })
}

fn invocation_admission(detail: impl std::fmt::Display) -> JvmInvocationError {
    JvmInvocationError::Admission(detail.to_string())
}

fn invocation_resource(detail: impl std::fmt::Display) -> JvmInvocationError {
    JvmInvocationError::Resource(detail.to_string())
}

/// Loadable JVM language library.
pub struct JvmLanguageLib {
    surface: Arc<JvmSurface>,
}
impl Default for JvmLanguageLib {
    fn default() -> Self {
        Self {
            surface: Arc::new(JvmSurface::new(1 << 20)),
        }
    }
}

impl Lib for JvmLanguageLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::new("sim/lang-jvm"),
            version: Version(env!("CARGO_PKG_VERSION").into()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: vec![],
            capabilities: vec![],
            exports: FunctionKind::ALL
                .into_iter()
                .map(|kind| Export::Function {
                    symbol: kind.symbol(),
                    function_id: None,
                })
                .collect(),
        }
    }
    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        for kind in FunctionKind::ALL {
            linker.function_value(
                kind.symbol(),
                cx.factory().opaque(Arc::new(JvmFunction {
                    surface: self.surface.clone(),
                    kind,
                }))?,
            )?;
        }
        Ok(())
    }
}

/// Installs the JVM language library idempotently.
pub fn install_jvm_language_lib(cx: &mut Cx) -> Result<()> {
    sim_lib_core::install_once_id(cx, &JvmLanguageLib::default()).map(|_| ())
}

#[derive(Clone, Copy)]
enum FunctionKind {
    Define,
    InvokeStatic,
    InvokeInstance,
    Browse,
    Profile,
    Fidelity,
}
impl FunctionKind {
    const ALL: [Self; 6] = [
        Self::Define,
        Self::InvokeStatic,
        Self::InvokeInstance,
        Self::Browse,
        Self::Profile,
        Self::Fidelity,
    ];
    fn symbol(self) -> Symbol {
        Symbol::qualified(
            "jvm",
            match self {
                Self::Define => "define",
                Self::InvokeStatic => "invoke-static",
                Self::InvokeInstance => "invoke-instance",
                Self::Browse => "browse",
                Self::Profile => "profile",
                Self::Fidelity => "fidelity",
            },
        )
    }
}
struct JvmFunction {
    surface: Arc<JvmSurface>,
    kind: FunctionKind,
}
impl Object for JvmFunction {
    fn display(&self, _: &mut Cx) -> Result<String> {
        Ok(format!("#<function {}>", self.kind.symbol()))
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
impl ObjectCompat for JvmFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}
impl Callable for JvmFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        self.call_values(cx, args.into_vec())
    }
    fn browse_args_shape(&self, cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(Some(cx.factory().opaque(Arc::new(AnyShape))?))
    }
    fn browse_result_shape(&self, cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(Some(cx.factory().opaque(Arc::new(AnyShape))?))
    }
}
impl JvmFunction {
    fn call_values(&self, cx: &mut Cx, values: Vec<Value>) -> Result<Value> {
        let exprs = values
            .iter()
            .map(|v| v.object().as_expr(cx))
            .collect::<Result<Vec<_>>>()?;
        match self.kind {
            FunctionKind::Define => {
                let [Expr::String(name), Expr::Bytes(bytes)] = exprs.as_slice() else {
                    return Err(Error::Eval(
                        "jvm/define expects class name and bytes".into(),
                    ));
                };
                self.surface.define(cx, name, bytes.clone())?;
                cx.factory().string(name.clone())
            }
            FunctionKind::InvokeStatic => {
                let [
                    Expr::String(class),
                    Expr::String(name),
                    Expr::String(desc),
                    rest @ ..,
                ] = exprs.as_slice()
                else {
                    return Err(Error::Eval("jvm/invoke-static expects class, method, descriptor, and integer arguments".into()));
                };
                let ints = rest.iter().map(as_i32).collect::<Result<Vec<_>>>()?;
                let result = self
                    .surface
                    .invoke_static_i32(cx, class, name, desc, &ints)
                    .map_err(Error::from)?;
                cx.factory()
                    .number_literal(Symbol::qualified("jvm", "int"), result.to_string())
            }
            FunctionKind::InvokeInstance => {
                let [
                    Expr::String(declaring),
                    Expr::String(receiver),
                    Expr::String(name),
                    Expr::String(desc),
                    rest @ ..,
                ] = exprs.as_slice()
                else {
                    return Err(Error::Eval("jvm/invoke-instance expects declaring class, receiver class, method, descriptor, and integer arguments".into()));
                };
                let ints = rest.iter().map(as_i32).collect::<Result<Vec<_>>>()?;
                let result = self
                    .surface
                    .invoke_instance_i32(cx, declaring, receiver, name, desc, &ints)
                    .map_err(Error::from)?;
                cx.factory()
                    .number_literal(Symbol::qualified("jvm", "int"), result.to_string())
            }
            FunctionKind::Browse => {
                let [limit] = exprs.as_slice() else {
                    return Err(Error::Eval("jvm/browse expects one bound".into()));
                };
                let rows = self.surface.browse(
                    cx,
                    usize::try_from(as_i32(limit)?)
                        .map_err(|_| Error::Eval("negative browse bound".into()))?,
                )?;
                cx.factory().expr(Expr::List(
                    rows.into_iter()
                        .map(|r| {
                            Expr::List(vec![
                                Expr::String(r.class),
                                Expr::List(r.methods.into_iter().map(Expr::String).collect()),
                                Expr::List(
                                    r.code_bytes
                                        .into_iter()
                                        .map(|n| Expr::String(n.to_string()))
                                        .collect(),
                                ),
                            ])
                        })
                        .collect(),
                ))
            }
            FunctionKind::Profile => cx
                .factory()
                .expr(Expr::List(jvm_language_profile().to_constructor_args())),
            FunctionKind::Fidelity => cx.factory().expr(Expr::List(
                JVM_DECLARED_ABSENCES
                    .into_iter()
                    .map(|v| Expr::Symbol(Symbol::qualified("jvm", v)))
                    .chain(std::iter::once(Expr::Symbol(Symbol::qualified(
                        "jvm",
                        "bounded-classfile-execution",
                    ))))
                    .collect(),
            )),
        }
    }
}
fn as_i32(expr: &Expr) -> Result<i32> {
    let Expr::Number(number) = expr else {
        return Err(Error::TypeMismatch {
            expected: "integer",
            found: "non-number",
        });
    };
    number
        .canonical
        .parse()
        .map_err(|_| Error::Eval("integer is outside JVM int range".into()))
}

#[cfg(test)]
include!("surface/entry_policy_tests.rs");
