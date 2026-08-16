//! Ordinary SIM-facing JVM callables, bounded browsing, and profile evidence.

use std::sync::Arc;

use sim_codec_classfile::{ByteReader, CodeAttribute, Constant, Opcode, decode_instructions};
use sim_kernel::{
    AbiVersion, Args, Callable, CapabilityName, ClassRef, Cx, Error, Export, Expr, Lib,
    LibManifest, LibTarget, Linker, Object, ObjectCompat, Result, ShapeRef, Symbol, Value, Version,
};
use sim_lib_standard_core::{FidelityBadge, LanguageProfile, OrganUse};
use sim_shape::AnyShape;

use crate::{ClassDefinition, ClassLoader, InvocationKind, select_invocation};

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
    /// Current heap object count. The public surface never exposes raw handles.
    pub heap_objects: usize,
}

/// Shared state behind the loadable JVM callables.
pub struct JvmSurface {
    loader: ClassLoader,
    frames: crate::JvmFramePool,
}

impl JvmSurface {
    /// Creates an isolated surface with a hard classfile byte allowance.
    pub fn new(max_classfile_bytes: usize) -> Self {
        Self {
            loader: ClassLoader::new(max_classfile_bytes),
            frames: crate::JvmFramePool::new(crate::JvmFramePoolPolicy {
                frames: 64,
                slots: 4_096,
                operands: 4_096,
            }),
        }
    }

    /// Defines caller-supplied bytes without consulting an ambient loader or transport.
    pub fn define(&self, cx: &mut Cx, name: &str, bytes: Vec<u8>) -> Result<Arc<ClassDefinition>> {
        self.loader.define_bytes(cx, name, bytes)
    }

    /// Invokes a bounded integer-only static method through exact JVM selection.
    pub fn invoke_static_i32(
        &self,
        cx: &mut Cx,
        class: &str,
        name: &str,
        descriptor: &str,
        args: &[i32],
    ) -> Result<i32> {
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
            return Err(Error::Eval(
                "instance method passed to jvm/invoke-static".into(),
            ));
        }
        execute_i32(&self.frames, &definition, name, descriptor, args, 0)
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
    ) -> Result<i32> {
        cx.require(&jvm_invoke_capability())?;
        let declared = self
            .loader
            .loaded(declaring)?
            .ok_or_else(|| Error::Eval(format!("JVM class {declaring} is not defined")))?;
        let resolved = crate::ConstantResolution {
            kind: crate::ConstantResolutionKind::Method,
            class: declared.id().clone(),
            name: Some(name.into()),
            descriptor: Some(descriptor.into()),
        };
        let selected = select_invocation(
            &self.loader,
            &resolved,
            InvocationKind::Virtual,
            Some(receiver),
        )
        .map_err(|error| Error::Eval(format!("JVM invocation failed: {error:?}")))?;
        execute_i32(
            &self.frames,
            selected.declaring_class(),
            name,
            descriptor,
            args,
            1,
        )
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
                    heap_objects: 0,
                })
            })
            .collect()
    }
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

fn execute_i32(
    frames: &crate::JvmFramePool,
    class: &ClassDefinition,
    name: &str,
    descriptor: &str,
    args: &[i32],
    local_offset: usize,
) -> Result<i32> {
    let index = class
        .metadata()
        .members()
        .iter()
        .filter(|m| matches!(m.kind(), crate::JavaMemberKind::Method))
        .position(|m| m.name() == name && m.descriptor() == descriptor)
        .ok_or_else(|| Error::Eval("selected JVM method body is missing".into()))?;
    let method = class
        .shell()
        .methods
        .get(index)
        .ok_or_else(|| Error::Eval("selected JVM method shell is missing".into()))?;
    let code = code_attribute(class, method)?
        .ok_or_else(|| Error::Eval("selected JVM method has no Code attribute".into()))?;
    let decoded = decode_instructions(
        &code.code,
        class.shell().major_version,
        &class.shell().constant_pool,
    )
    .map_err(|e| Error::Eval(e.to_string()))?;
    let mut lease = frames.acquire(usize::from(code.max_locals), usize::from(code.max_stack));
    for (slot, value) in args.iter().copied().enumerate() {
        let slot = slot + local_offset;
        if slot < lease.frame().locals().limit() {
            lease
                .frame_mut()
                .locals_mut()
                .store(slot, crate::JvmValue::Int(value))
                .map_err(|error| {
                    Error::Eval(format!("JVM local initialization failed: {error:?}"))
                })?;
        }
    }
    for instruction in decoded.instructions {
        let opcode = instruction.instruction.opcode;
        match opcode {
            Opcode::Iload0 | Opcode::Iload1 | Opcode::Iload2 | Opcode::Iload3 => {
                let slot = usize::from(opcode as u8 - Opcode::Iload0 as u8);
                let crate::JvmValue::Int(value) = lease
                    .frame()
                    .locals()
                    .load(slot)
                    .map_err(|_| Error::Eval("JVM integer local missing".into()))?
                else {
                    return Err(Error::Eval("JVM integer local has wrong category".into()));
                };
                let value = *value;
                lease
                    .frame_mut()
                    .operands_mut()
                    .push(crate::JvmValue::Int(value))
                    .map_err(|error| Error::Eval(format!("JVM operand push failed: {error:?}")))?;
            }
            Opcode::Istore0 | Opcode::Istore1 | Opcode::Istore2 | Opcode::Istore3 => {
                let slot = usize::from(opcode as u8 - Opcode::Istore0 as u8);
                let value = pop_i32(&mut lease)?;
                lease
                    .frame_mut()
                    .locals_mut()
                    .store(slot, crate::JvmValue::Int(value))
                    .map_err(|_| Error::Eval("JVM integer local missing".into()))?;
            }
            Opcode::IconstM1
            | Opcode::Iconst0
            | Opcode::Iconst1
            | Opcode::Iconst2
            | Opcode::Iconst3
            | Opcode::Iconst4
            | Opcode::Iconst5 => lease
                .frame_mut()
                .operands_mut()
                .push(crate::JvmValue::Int(opcode as i32 - Opcode::Iconst0 as i32))
                .map_err(|error| Error::Eval(format!("JVM operand push failed: {error:?}")))?,
            Opcode::Iadd | Opcode::Isub | Opcode::Imul => {
                let right = pop_i32(&mut lease)?;
                let left = pop_i32(&mut lease)?;
                let value = match opcode {
                    Opcode::Iadd => left.wrapping_add(right),
                    Opcode::Isub => left.wrapping_sub(right),
                    _ => left.wrapping_mul(right),
                };
                lease
                    .frame_mut()
                    .operands_mut()
                    .push(crate::JvmValue::Int(value))
                    .map_err(|error| Error::Eval(format!("JVM operand push failed: {error:?}")))?;
            }
            Opcode::Ireturn => {
                let value = pop_i32(&mut lease)
                    .map_err(|_| Error::Eval("JVM return operand missing".into()))?;
                lease.complete();
                return Ok(value);
            }
            other => {
                return Err(Error::Eval(format!(
                    "JVM callable subset refuses opcode {other:?}"
                )));
            }
        }
    }
    Err(Error::Eval("JVM method completed without ireturn".into()))
}

fn pop_i32(lease: &mut crate::JvmFrameLease) -> Result<i32> {
    match lease
        .frame_mut()
        .operands_mut()
        .pop()
        .map_err(|_| Error::Eval("JVM operand underflow".into()))?
    {
        crate::JvmValue::Int(value) => Ok(value),
        _ => Err(Error::Eval("JVM integer operand has wrong category".into())),
    }
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
                    .invoke_static_i32(cx, class, name, desc, &ints)?;
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
                    .invoke_instance_i32(cx, declaring, receiver, name, desc, &ints)?;
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
                                Expr::String(r.heap_objects.to_string()),
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
