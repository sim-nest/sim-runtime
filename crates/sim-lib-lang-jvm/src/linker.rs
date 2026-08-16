//! Exact, revision-bound identity and state for JVM bootstrap linkage sites.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Weak},
};

use sim_kernel::{
    Args, Callable, CapabilityName, ClassId, ClassRef, Cx, Error, Object, ObjectCompat, Ref,
    ShapeRef, Symbol, Value,
};
use sim_lib_class::{
    ClassDescriptor, ClassDescriptorInput, ClassIdentity, DeclaredParent, DescriptorClass,
    MemberShape, OpenMetadataEntry,
};
use sim_lib_function::{
    BoundCall, CapturedBinding, FunctionBodyPolicy, FunctionInstance, FunctionPlan,
};
use sim_lib_mutation::{ManagedHandle, RootedHandle};
use sim_shape::AnyShape;

use crate::{
    ClassDefinition, ClassDefinitionId, ClassLoader, ClassSpaceRevision, ConstantResolutionError,
    ConstantResolutionKind, InvocationError, JavaMember, JvmGraphError, JvmHeap, JvmValue,
    ResolutionCache,
};

/// Capability required to project a SIM callable as a Java functional interface.
///
/// Calling an already linked Java lambda from SIM uses [`crate::jvm_invoke_capability`].
/// Manufacturing a new loader-owned Java class is a distinct authority boundary.
pub fn jvm_functional_adapter_capability() -> CapabilityName {
    CapabilityName::new("jvm.functional-adapter")
}

/// Java-facing completion from a linked lambda invoked through ordinary SIM `Callable`.
#[derive(Clone, Debug)]
pub enum JavaLambdaCallOutcome {
    /// The SAM returned one value.
    Returned(JvmValue),
    /// The JVM stopped at a safepoint. The continuation remains owned by the JVM caller.
    Interrupted,
    /// Java raised a throwable; its stable Java-facing diagnostic is preserved.
    Threw(String),
}

/// A linked Java lambda projected through the kernel `FUNCTION_2` callable boundary.
pub struct JavaLambdaCallable {
    argument_shapes: Vec<ShapeRef>,
    result_shape: Option<ShapeRef>,
    invoke: Arc<dyn Fn(&mut Cx, Vec<JvmValue>) -> JavaLambdaCallOutcome + Send + Sync>,
}

impl JavaLambdaCallable {
    /// Creates a projection over an already linked, rooted Java lambda target.
    pub fn new(
        argument_shapes: Vec<ShapeRef>,
        result_shape: Option<ShapeRef>,
        invoke: impl Fn(&mut Cx, Vec<JvmValue>) -> JavaLambdaCallOutcome + Send + Sync + 'static,
    ) -> Self {
        Self {
            argument_shapes,
            result_shape,
            invoke: Arc::new(invoke),
        }
    }
}

impl Object for JavaLambdaCallable {
    fn display(&self, _cx: &mut Cx) -> sim_kernel::Result<String> {
        Ok("#<jvm-lambda>".into())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for JavaLambdaCallable {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for JavaLambdaCallable {
    fn call(&self, cx: &mut Cx, args: Args) -> sim_kernel::Result<Value> {
        cx.require(&crate::jvm_invoke_capability())?;
        let arguments = args.into_vec();
        if arguments.len() != self.argument_shapes.len() {
            return Err(Error::Eval(format!(
                "JVM lambda Shape arity mismatch: expected {}, got {}",
                self.argument_shapes.len(),
                arguments.len()
            )));
        }
        for (index, (shape, value)) in self
            .argument_shapes
            .iter()
            .zip(arguments.iter())
            .enumerate()
        {
            let Some(checker) = shape.object().as_shape() else {
                return Err(Error::Eval(format!(
                    "JVM lambda argument {index} Shape is not a Shape"
                )));
            };
            if !checker.check_value(cx, value.clone())?.accepted {
                return Err(Error::Eval(format!(
                    "JVM lambda argument {index} failed its Shape"
                )));
            }
        }
        let values = arguments.into_iter().map(JvmValue::Kernel).collect();
        match (self.invoke)(cx, values) {
            JavaLambdaCallOutcome::Returned(JvmValue::Kernel(value)) => {
                if let Some(shape) = &self.result_shape {
                    let Some(checker) = shape.object().as_shape() else {
                        return Err(Error::Eval("JVM lambda result Shape is not a Shape".into()));
                    };
                    if !checker.check_value(cx, value.clone())?.accepted {
                        return Err(Error::Eval("JVM lambda result failed its Shape".into()));
                    }
                }
                Ok(value)
            }
            JavaLambdaCallOutcome::Returned(_) => Err(Error::Eval(
                "JVM lambda returned a non-SIM value without descriptor conversion".into(),
            )),
            JavaLambdaCallOutcome::Interrupted => Err(Error::Eval(
                "JVM lambda interrupted; resume it through the JVM continuation".into(),
            )),
            JavaLambdaCallOutcome::Threw(error) => {
                Err(Error::Eval(format!("Java lambda threw: {error}")))
            }
        }
    }

    fn browse_args_shape(&self, _cx: &mut Cx) -> sim_kernel::Result<Option<ShapeRef>> {
        Ok((self.argument_shapes.len() == 1).then(|| self.argument_shapes[0].clone()))
    }

    fn browse_result_shape(&self, _cx: &mut Cx) -> sim_kernel::Result<Option<ShapeRef>> {
        Ok(self.result_shape.clone())
    }
}

/// An admitted SIM callable paired with its managed generated Java class.
pub struct SimFunctionalAdapter {
    callable: Value,
    class: Arc<GeneratedLambdaClass>,
}

impl SimFunctionalAdapter {
    /// Loader-owned generated class implementing the admitted SAM.
    pub fn generated_class(&self) -> &Arc<GeneratedLambdaClass> {
        &self.class
    }

    /// Invokes the SIM function from Java after descriptor conversion to kernel values.
    pub fn invoke(&self, cx: &mut Cx, arguments: Vec<JvmValue>) -> JavaLambdaCallOutcome {
        let mut converted = Vec::with_capacity(arguments.len());
        for argument in arguments {
            let JvmValue::Kernel(value) = argument else {
                return JavaLambdaCallOutcome::Threw(
                    "functional-interface argument lacks admitted descriptor conversion".into(),
                );
            };
            converted.push(value);
        }
        let Some(callable) = self.callable.object().as_callable() else {
            return JavaLambdaCallOutcome::Threw("adapted SIM value is no longer Callable".into());
        };
        match callable.call(cx, Args::new(converted)) {
            Ok(value) => JavaLambdaCallOutcome::Returned(JvmValue::Kernel(value)),
            Err(error) => JavaLambdaCallOutcome::Threw(error.to_string()),
        }
    }
}

/// Admits a SIM callable as a Java SAM, generating its managed class only after authority checks.
///
/// `generate` must perform functional-interface discovery, descriptor validation, and
/// loader-local class construction. It is deliberately not called on any refusal path.
pub fn adapt_sim_callable_as_functional_interface(
    cx: &mut Cx,
    callable: Value,
    generate: impl FnOnce() -> Result<Arc<GeneratedLambdaClass>, FunctionalInterfaceError>,
) -> Result<SimFunctionalAdapter, FunctionalInterfaceError> {
    cx.require(&jvm_functional_adapter_capability())
        .map_err(|error| FunctionalInterfaceError::InteropRefused(error.to_string()))?;
    if callable.object().as_callable().is_none() {
        return Err(FunctionalInterfaceError::InteropRefused(
            "SIM value does not project FUNCTION_2 Callable".into(),
        ));
    }
    let class = generate()?;
    Ok(SimFunctionalAdapter { callable, class })
}

/// Receiver placement retained by a resolved direct implementation handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectReceiver {
    /// A static implementation or constructor has no pre-existing receiver.
    None,
    /// The receiver is captured when the lambda instance is created.
    Bound,
    /// The receiver is supplied as the first SAM invocation argument.
    Unbound,
}

/// Exact invocation semantics of an admitted `CONSTANT_MethodHandle` kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectInvocationKind {
    /// `REF_invokeStatic` (6).
    Static,
    /// `REF_newInvokeSpecial` (8).
    Constructor,
    /// `REF_invokeSpecial` (7).
    Special,
    /// `REF_invokeVirtual` (5).
    Virtual,
    /// `REF_invokeInterface` (9).
    Interface,
}

/// The part of a lambda call at which a descriptor adaptation is performed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AdaptationPoint {
    /// A value captured while the lambda object is created.
    Capture(usize),
    /// The bound or unbound implementation receiver.
    Receiver,
    /// A SAM argument supplied when the lambda is invoked.
    Parameter(usize),
    /// The implementation result returned to the SAM caller.
    Return,
}

/// One immutable JVM conversion selected while linking a lambda.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JvmAdaptation {
    /// No representation or type change is required.
    Identity,
    /// A checked reference conversion is required at invocation.
    ReferenceCast {
        /// Runtime source descriptor.
        from: String,
        /// Required target descriptor.
        to: String,
    },
    /// A Java widening primitive conversion.
    PrimitiveWiden {
        /// Source primitive descriptor.
        from: char,
        /// Wider target primitive descriptor.
        to: char,
    },
    /// Box a primitive, optionally followed by a reference cast.
    Box {
        /// Primitive descriptor to box.
        primitive: char,
        /// Wrapper or widened reference descriptor.
        reference: String,
    },
    /// Unbox a wrapper, optionally followed by primitive widening.
    Unbox {
        /// Wrapper reference descriptor.
        reference: String,
        /// Required primitive descriptor after optional widening.
        primitive: char,
    },
    /// Discard an implementation value for a void SAM result.
    DropValue,
}

/// One located conversion in an immutable JVM function policy body.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocatedJvmAdaptation {
    /// Stable position at which the conversion will execute.
    pub point: AdaptationPoint,
    /// Conversion compiled for that position.
    pub adaptation: JvmAdaptation,
}

/// JVM-owned policy composed with the language-neutral function organ.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JvmFunctionPolicyBody {
    adaptations: Box<[LocatedJvmAdaptation]>,
}

impl JvmFunctionPolicyBody {
    /// Returns the complete, execution-order adaptation program.
    pub fn adaptations(&self) -> &[LocatedJvmAdaptation] {
        &self.adaptations
    }
}

impl FunctionBodyPolicy for JvmFunctionPolicyBody {
    fn invoke(
        &self,
        _cx: &mut Cx,
        _plan: &FunctionPlan,
        _captures: &[CapturedBinding],
        _call: BoundCall,
    ) -> sim_kernel::Result<Value> {
        Err(Error::Eval(
            "JVM lambda invocation is not installed until the SAM linker phase".into(),
        ))
    }
}

/// A neutral declaration paired with its immutable JVM linkage policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JvmFunctionPlan {
    neutral: FunctionPlan,
    body: JvmFunctionPolicyBody,
}

impl JvmFunctionPlan {
    /// Returns the language-neutral declaration metadata unchanged.
    pub const fn neutral(&self) -> &FunctionPlan {
        &self.neutral
    }

    /// Returns the JVM-owned body policy.
    pub const fn body(&self) -> &JvmFunctionPolicyBody {
        &self.body
    }
}

/// Exact generated member selected for one lambda invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SelectedLambdaMember {
    name: String,
    descriptor: String,
    role: GeneratedLambdaMemberRole,
}

impl SelectedLambdaMember {
    /// JVM member name used for selection.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// JVM descriptor used for selection and bridge erasure.
    pub fn descriptor(&self) -> &str {
        &self.descriptor
    }

    /// Whether this is the SAM declaration or an ordinary bridge declaration.
    pub const fn role(&self) -> GeneratedLambdaMemberRole {
        self.role
    }
}

/// Immutable input handed to the ordinary JVM method pipeline.
pub struct LambdaMethodCall<'a, R> {
    /// Generated SAM or bridge selected by exact JVM name and descriptor.
    pub member: SelectedLambdaMember,
    /// Access-checked implementation method-handle target.
    pub implementation: &'a ResolvedDirectHandle,
    /// Frozen, execution-ordered conversion program.
    pub adaptations: &'a [LocatedJvmAdaptation],
    /// Captured values in factory descriptor order.
    pub captures: &'a [JvmValue],
    /// Invocation values in the selected member descriptor order.
    pub arguments: Vec<JvmValue>,
    /// Pipeline-owned continuation supplied when resuming an interrupted call.
    pub resume: Option<R>,
}

/// Completion channels preserved from the ordinary JVM method pipeline.
#[derive(Clone, Debug)]
pub enum LambdaInvocationOutcome<R, E> {
    /// The implementation returned normally, with its exact accumulated work charge.
    Returned {
        /// Value produced by the implementation descriptor, if any.
        value: Option<JvmValue>,
        /// Exact cumulative instruction charge.
        work: usize,
    },
    /// The implementation threw through the shared raised envelope.
    Threw {
        /// Unmodified shared exception envelope.
        exception: E,
        /// Exact cumulative instruction charge.
        work: usize,
    },
    /// The implementation stopped at a safepoint with an exact resumable continuation.
    Interrupted {
        /// Pipeline-owned continuation evidence.
        resume: R,
        /// Exact cumulative instruction charge before the safepoint stop.
        work: usize,
    },
}

/// The existing JVM method executor consumed by lambda linkage.
///
/// Implementations apply `call.adaptations`, construct the same machine call
/// transfer used by ordinary Java invocation, and retain their native exception,
/// work, safepoint, and continuation contracts. The linker never drives a second
/// bytecode loop.
pub trait LambdaMethodPipeline {
    /// Pipeline-owned continuation evidence.
    type Resume;
    /// Shared exception-envelope type.
    type Exception;

    /// Invokes or resumes one already-resolved implementation method.
    fn invoke(
        &mut self,
        call: LambdaMethodCall<'_, Self::Resume>,
    ) -> Result<LambdaInvocationOutcome<Self::Resume, Self::Exception>, InvocationError>;
}

/// Failure stage for compiling JVM descriptor adaptation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JvmAdaptationError {
    /// One of the supplied method descriptors is malformed.
    InvalidDescriptor {
        /// Descriptor's linkage role.
        role: &'static str,
        /// Refused descriptor text.
        descriptor: String,
    },
    /// Neutral capture or parameter declarations disagree with JVM arity.
    NeutralArity {
        /// Neutral declaration role that disagreed.
        role: &'static str,
        /// Number declared by the neutral plan.
        neutral: usize,
        /// Number encoded by the JVM descriptor.
        descriptor: usize,
    },
    /// Captures, receiver, and invocation arguments do not fill the target method.
    ImplementationArity {
        /// Values available after receiver placement.
        supplied: usize,
        /// Implementation descriptor arity.
        required: usize,
    },
    /// Receiver placement and the supplied receiver descriptor disagree.
    ReceiverDescriptor {
        /// Selected receiver placement.
        receiver: DirectReceiver,
        /// Whether a receiver target descriptor was supplied.
        supplied: bool,
    },
    /// Java's lambda conversion rules reject the located conversion.
    UnsupportedConversion {
        /// Exact conversion location.
        point: AdaptationPoint,
        /// Source descriptor.
        from: String,
        /// Target descriptor.
        to: String,
    },
    /// A void implementation cannot produce a value required by the SAM.
    VoidToValue {
        /// Exact result conversion location.
        point: AdaptationPoint,
        /// Value descriptor required by the SAM.
        required: String,
    },
}

/// Compiles all lambda placement and conversion decisions before allocation or invocation.
pub fn compile_jvm_function_plan(
    neutral: FunctionPlan,
    factory_descriptor: &str,
    invocation_descriptor: &str,
    implementation_descriptor: &str,
    receiver: DirectReceiver,
    receiver_descriptor: Option<&str>,
) -> Result<JvmFunctionPlan, JvmAdaptationError> {
    let (captures, factory_result) = adaptation_descriptor("factory", factory_descriptor)?;
    let (parameters, invocation_result) =
        adaptation_descriptor("invocation", invocation_descriptor)?;
    let (implementation, implementation_result) =
        adaptation_descriptor("implementation", implementation_descriptor)?;
    if neutral.captures().len() != captures.len() {
        return Err(JvmAdaptationError::NeutralArity {
            role: "capture",
            neutral: neutral.captures().len(),
            descriptor: captures.len(),
        });
    }
    if neutral.parameters().len() != parameters.len() {
        return Err(JvmAdaptationError::NeutralArity {
            role: "parameter",
            neutral: neutral.parameters().len(),
            descriptor: parameters.len(),
        });
    }
    if !is_reference(&factory_result) {
        return Err(JvmAdaptationError::UnsupportedConversion {
            point: AdaptationPoint::Return,
            from: factory_result,
            to: "lambda reference".into(),
        });
    }

    let receiver_source = match receiver {
        DirectReceiver::None => None,
        DirectReceiver::Bound => captures.first().cloned(),
        DirectReceiver::Unbound => parameters.first().cloned(),
    };
    if receiver_source.is_some() != receiver_descriptor.is_some() {
        return Err(JvmAdaptationError::ReceiverDescriptor {
            receiver,
            supplied: receiver_descriptor.is_some(),
        });
    }
    let capture_start = usize::from(receiver == DirectReceiver::Bound);
    let parameter_start = usize::from(receiver == DirectReceiver::Unbound);
    let supplied = captures.len().saturating_sub(capture_start)
        + parameters.len().saturating_sub(parameter_start);
    if supplied != implementation.len() {
        return Err(JvmAdaptationError::ImplementationArity {
            supplied,
            required: implementation.len(),
        });
    }

    let mut sources = Vec::with_capacity(supplied);
    sources.extend(
        captures[capture_start..]
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, ty)| (AdaptationPoint::Capture(index + capture_start), ty)),
    );
    sources.extend(
        parameters[parameter_start..]
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, ty)| (AdaptationPoint::Parameter(index + parameter_start), ty)),
    );
    let mut adaptations = sources
        .into_iter()
        .zip(implementation)
        .map(|((point, from), to)| {
            compile_conversion(point, &from, &to)
                .map(|adaptation| LocatedJvmAdaptation { point, adaptation })
        })
        .collect::<Result<Vec<_>, _>>()?;
    if let (Some(from), Some(to)) = (receiver_source, receiver_descriptor) {
        adaptations.insert(
            0,
            LocatedJvmAdaptation {
                point: AdaptationPoint::Receiver,
                adaptation: compile_conversion(AdaptationPoint::Receiver, &from, to)?,
            },
        );
    }
    let result_adaptation = if invocation_result == "V" {
        (implementation_result != "V").then_some(JvmAdaptation::DropValue)
    } else if implementation_result == "V" {
        return Err(JvmAdaptationError::VoidToValue {
            point: AdaptationPoint::Return,
            required: invocation_result,
        });
    } else {
        Some(compile_conversion(
            AdaptationPoint::Return,
            &implementation_result,
            &invocation_result,
        )?)
    };
    if let Some(adaptation) = result_adaptation {
        adaptations.push(LocatedJvmAdaptation {
            point: AdaptationPoint::Return,
            adaptation,
        });
    }
    Ok(JvmFunctionPlan {
        neutral,
        body: JvmFunctionPolicyBody {
            adaptations: adaptations.into_boxed_slice(),
        },
    })
}

fn adaptation_descriptor(
    role: &'static str,
    descriptor: &str,
) -> Result<(Vec<String>, String), JvmAdaptationError> {
    split_method_descriptor(descriptor).map_err(|_| JvmAdaptationError::InvalidDescriptor {
        role,
        descriptor: descriptor.into(),
    })
}

fn compile_conversion(
    point: AdaptationPoint,
    from: &str,
    to: &str,
) -> Result<JvmAdaptation, JvmAdaptationError> {
    if from == to {
        return Ok(JvmAdaptation::Identity);
    }
    if is_reference(from) && is_reference(to) {
        return Ok(JvmAdaptation::ReferenceCast {
            from: from.into(),
            to: to.into(),
        });
    }
    if let (Some(from), Some(to)) = (primitive(from), primitive(to)) {
        if primitive_widens(from, to) {
            return Ok(JvmAdaptation::PrimitiveWiden { from, to });
        }
    }
    if let (Some(primitive), true) = (primitive(from), is_reference(to)) {
        if wrapper_primitive(to).is_some_and(|wrapped| wrapped == primitive)
            || to == "Ljava/lang/Object;"
        {
            return Ok(JvmAdaptation::Box {
                primitive,
                reference: to.into(),
            });
        }
    }
    if is_reference(from)
        && let (Some(unboxed), Some(target)) = (wrapper_primitive(from), primitive(to))
        && (unboxed == target || primitive_widens(unboxed, target))
    {
        return Ok(JvmAdaptation::Unbox {
            reference: from.into(),
            primitive: target,
        });
    }
    Err(JvmAdaptationError::UnsupportedConversion {
        point,
        from: from.into(),
        to: to.into(),
    })
}

fn primitive(descriptor: &str) -> Option<char> {
    (descriptor.len() == 1)
        .then(|| descriptor.chars().next().unwrap())
        .filter(|value| matches!(value, 'B' | 'C' | 'D' | 'F' | 'I' | 'J' | 'S' | 'Z'))
}

fn primitive_widens(from: char, to: char) -> bool {
    matches!(
        (from, to),
        ('B', 'S' | 'I' | 'J' | 'F' | 'D')
            | ('S' | 'C', 'I' | 'J' | 'F' | 'D')
            | ('I', 'J' | 'F' | 'D')
            | ('J', 'F' | 'D')
            | ('F', 'D')
    )
}

fn wrapper_primitive(descriptor: &str) -> Option<char> {
    Some(match descriptor {
        "Ljava/lang/Boolean;" => 'Z',
        "Ljava/lang/Byte;" => 'B',
        "Ljava/lang/Character;" => 'C',
        "Ljava/lang/Short;" => 'S',
        "Ljava/lang/Integer;" => 'I',
        "Ljava/lang/Long;" => 'J',
        "Ljava/lang/Float;" => 'F',
        "Ljava/lang/Double;" => 'D',
        _ => return None,
    })
}

/// Access-checked, loader-bound implementation target retained by lambda linkage.
#[derive(Clone, Debug)]
pub struct ResolvedDirectHandle {
    kind: DirectInvocationKind,
    declaring_class: Arc<ClassDefinition>,
    method: JavaMember,
    receiver: DirectReceiver,
}

impl ResolvedDirectHandle {
    /// Exact admitted reference-kind semantics.
    pub const fn kind(&self) -> DirectInvocationKind {
        self.kind
    }
    /// Content- and loader-bound declaration owner.
    pub fn declaring_class(&self) -> &Arc<ClassDefinition> {
        &self.declaring_class
    }
    /// Exact declaration selected by symbolic method resolution.
    pub fn method(&self) -> &JavaMember {
        &self.method
    }
    /// Whether the receiver is absent, captured, or supplied at invocation.
    pub const fn receiver(&self) -> DirectReceiver {
        self.receiver
    }
    /// Whether invocation is an active use that must trigger class initialization.
    pub const fn initializes_on_invocation(&self) -> bool {
        matches!(
            self.kind,
            DirectInvocationKind::Static | DirectInvocationKind::Constructor
        )
    }
}

/// Stable failure stage for resolving a direct lambda implementation handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DirectHandleError {
    /// The reference kind is not one of the five invocable direct kinds.
    UnsupportedReferenceKind(u8),
    /// Normative symbolic resolution, including access checking, failed.
    Resolution(ConstantResolutionError),
    /// Managed resolution-cache bookkeeping failed.
    Managed(String),
    /// The resolved constant or declaration contradicts the reference kind.
    KindMismatch,
    /// An instance handle omitted an explicit bound/unbound receiver rule.
    MissingReceiverRule,
    /// A static handle incorrectly carried a receiver.
    UnexpectedReceiver,
}

/// Resolves one implementation handle through the JVM's access-checked method resolver.
///
/// This performs no class initialization. The returned product records whether
/// invocation is an active use, leaving the initialization trigger in the
/// invocation pipeline where JVMS 5.5 requires it.
#[allow(clippy::too_many_arguments)]
pub fn resolve_direct_handle(
    resolution_cache: &ResolutionCache,
    heap: &mut JvmHeap,
    cache_handle: ManagedHandle,
    owner_handle: ManagedHandle,
    loader: &ClassLoader,
    owner: &Arc<ClassDefinition>,
    constant_pool_index: u16,
    reference_kind: u8,
    receiver: DirectReceiver,
) -> Result<ResolvedDirectHandle, DirectHandleError> {
    let kind = match reference_kind {
        5 => DirectInvocationKind::Virtual,
        6 => DirectInvocationKind::Static,
        7 => DirectInvocationKind::Special,
        8 => DirectInvocationKind::Constructor,
        9 => DirectInvocationKind::Interface,
        other => return Err(DirectHandleError::UnsupportedReferenceKind(other)),
    };
    let resolved = resolution_cache
        .resolve(
            heap,
            cache_handle,
            owner_handle,
            loader,
            owner,
            constant_pool_index,
        )
        .map_err(|error: JvmGraphError| DirectHandleError::Managed(format!("{error:?}")))?
        .map_err(DirectHandleError::Resolution)?;
    let expected_constant_kind = if kind == DirectInvocationKind::Interface {
        ConstantResolutionKind::InterfaceMethod
    } else {
        ConstantResolutionKind::Method
    };
    if resolved.kind != expected_constant_kind {
        return Err(DirectHandleError::KindMismatch);
    }
    let declaring_class = loader
        .loaded(resolved.class.binary_name())
        .map_err(|error| DirectHandleError::Managed(error.to_string()))?
        .filter(|class| class.id() == &resolved.class)
        .ok_or(DirectHandleError::KindMismatch)?;
    let name = resolved
        .name
        .as_deref()
        .ok_or(DirectHandleError::KindMismatch)?;
    let descriptor = resolved
        .descriptor
        .as_deref()
        .ok_or(DirectHandleError::KindMismatch)?;
    let method = declaring_class
        .metadata()
        .select_method(name, descriptor)
        .cloned()
        .ok_or(DirectHandleError::KindMismatch)?;
    match kind {
        DirectInvocationKind::Static if receiver != DirectReceiver::None => {
            return Err(DirectHandleError::UnexpectedReceiver);
        }
        DirectInvocationKind::Static if !method.is_static() => {
            return Err(DirectHandleError::KindMismatch);
        }
        DirectInvocationKind::Constructor
            if name != "<init>" || method.is_static() || receiver != DirectReceiver::None =>
        {
            return Err(DirectHandleError::KindMismatch);
        }
        DirectInvocationKind::Special
        | DirectInvocationKind::Virtual
        | DirectInvocationKind::Interface
            if method.is_static() =>
        {
            return Err(DirectHandleError::KindMismatch);
        }
        DirectInvocationKind::Special
        | DirectInvocationKind::Virtual
        | DirectInvocationKind::Interface
            if receiver == DirectReceiver::None =>
        {
            return Err(DirectHandleError::MissingReceiverRule);
        }
        _ => {}
    }
    Ok(ResolvedDirectHandle {
        kind,
        declaring_class,
        method,
        receiver,
    })
}

/// Shape of the variable portion of an admitted lambda bootstrap payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LambdaProtocolTail {
    /// The protocol has exactly its three standard arguments.
    None,
    /// The protocol has flags followed by flag-governed counted sections.
    FlagGoverned,
}

/// One manifest-derived lambda bootstrap identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LambdaBootstrapProtocol {
    /// Bootstrap owner internal name.
    pub owner: &'static str,
    /// Bootstrap member name.
    pub name: &'static str,
    /// Exact bootstrap descriptor.
    pub descriptor: &'static str,
    /// Payload tail shape.
    pub tail: LambdaProtocolTail,
}

struct LambdaBootstrapRegistry {
    protocols: &'static [LambdaBootstrapProtocol],
    admitted_flags_mask: i32,
    reference_kinds: &'static [i64],
}

static LAMBDA_BOOTSTRAP_REGISTRY: LambdaBootstrapRegistry =
    include!(concat!(env!("OUT_DIR"), "/jvm_lambda_protocols.rs"));

/// Returns the executor's manifest-derived admitted lambda protocol set.
pub fn executor_admitted_lambda_protocols() -> &'static [LambdaBootstrapProtocol] {
    LAMBDA_BOOTSTRAP_REGISTRY.protocols
}

/// A resolved constant-pool bootstrap argument ready for protocol validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedBootstrapArgument {
    /// A method-type descriptor.
    MethodType(String),
    /// A method handle and its JVMS reference kind.
    MethodHandle {
        /// JVMS `reference_kind` from the resolved `CONSTANT_MethodHandle`.
        reference_kind: u8,
    },
    /// A marker-interface class internal name.
    Class(String),
    /// An integer flag or count.
    Integer(i32),
}

/// Fully validated lambda bootstrap payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LambdaBootstrapPlan {
    /// Erased SAM method descriptor.
    pub sam_method_type: String,
    /// Implementation method-handle reference kind.
    pub implementation_reference_kind: u8,
    /// Instantiated SAM method descriptor.
    pub instantiated_method_type: String,
    /// Marker interfaces requested by `altMetafactory`.
    pub marker_interfaces: Vec<String>,
    /// Additional bridge method descriptors.
    pub bridges: Vec<String>,
    /// Whether serialization support was requested.
    pub serializable: bool,
}

/// Declaration role of one member on a generated lambda class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedLambdaMemberRole {
    /// Capturing factory constructor whose arguments become immutable captures.
    FactoryConstructor,
    /// Concrete implementation of the functional interface's single abstract method.
    Sam,
    /// Erasure bridge requested by `altMetafactory`.
    Bridge,
}

/// Exact JVM declaration retained for a byte-free generated lambda member.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedLambdaMember {
    name: String,
    descriptor: String,
    role: GeneratedLambdaMemberRole,
}

impl GeneratedLambdaMember {
    /// JVM member name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// JVM method descriptor.
    pub fn descriptor(&self) -> &str {
        &self.descriptor
    }

    /// Linker role represented by this declaration.
    pub const fn role(&self) -> GeneratedLambdaMemberRole {
        self.role
    }
}

/// A loader-bound lambda class assembled directly from checked metadata.
///
/// Generated classes deliberately have no classfile expression, shell, or byte
/// storage. Their neutral face is the same checked `CLASS_2` descriptor used by
/// loaded classes, while invocation policy remains in the linker.
#[derive(Clone, Debug)]
pub struct GeneratedLambdaClass {
    binary_name: String,
    loader: crate::ClassLoaderId,
    mirror: ManagedHandle,
    descriptor: ClassDescriptor,
    members: Vec<GeneratedLambdaMember>,
    serializable: bool,
}

impl GeneratedLambdaClass {
    /// Stable generated binary name within the capturing loader.
    pub fn binary_name(&self) -> &str {
        &self.binary_name
    }

    /// Capturing loader which owns this class identity.
    pub const fn loader(&self) -> crate::ClassLoaderId {
        self.loader
    }

    /// Managed class-mirror node allocated for this class.
    pub const fn mirror(&self) -> ManagedHandle {
        self.mirror
    }

    /// Neutral browsable class metadata.
    pub fn descriptor(&self) -> &ClassDescriptor {
        &self.descriptor
    }

    /// JVM linker declarations in factory, SAM, then bridge order.
    pub fn members(&self) -> &[GeneratedLambdaMember] {
        &self.members
    }

    /// Whether the checked bootstrap requested Java lambda serialization.
    pub const fn serializable(&self) -> bool {
        self.serializable
    }

    /// Selects a callable lambda member by the JVM's exact name-and-descriptor key.
    ///
    /// Factory constructors are not callable through a lambda instance. Bridges
    /// otherwise receive no special ranking or fallback treatment.
    pub fn select_invocation_member(
        &self,
        name: &str,
        descriptor: &str,
    ) -> Option<SelectedLambdaMember> {
        self.members
            .iter()
            .find(|member| {
                member.role != GeneratedLambdaMemberRole::FactoryConstructor
                    && member.name == name
                    && member.descriptor == descriptor
            })
            .map(|member| SelectedLambdaMember {
                name: member.name.clone(),
                descriptor: member.descriptor.clone(),
                role: member.role,
            })
    }

    /// Projects this generated definition as an ordinary Shape-bearing class.
    pub fn class_value(
        &self,
        cx: &Cx,
        lineage_nodes: usize,
        lineage_work: usize,
    ) -> sim_kernel::Result<Value> {
        cx.factory().opaque(Arc::new(DescriptorClass::new(
            self.descriptor.clone(),
            |_cx: &mut Cx, _args| {
                Err(Error::Eval(
                    "generated JVM lambda instances require linker invocation".into(),
                ))
            },
            lineage_nodes,
            lineage_work,
        )))
    }
}

/// Selects a generated SAM/bridge and invokes its resolved implementation through
/// the caller's one JVM method pipeline.
///
/// A resumed call repeats selection against the immutable generated class and
/// passes the continuation through unchanged. Consequently lambda linkage cannot
/// reorder Java handlers, lose work evidence, or invent a distinct safepoint
/// contract.
#[allow(clippy::too_many_arguments)]
pub fn invoke_lambda_member<P: LambdaMethodPipeline>(
    pipeline: &mut P,
    class: &GeneratedLambdaClass,
    plan: &JvmFunctionPlan,
    implementation: &ResolvedDirectHandle,
    name: &str,
    descriptor: &str,
    captures: &[JvmValue],
    arguments: Vec<JvmValue>,
    resume: Option<P::Resume>,
) -> Result<LambdaInvocationOutcome<P::Resume, P::Exception>, InvocationError> {
    let member = class
        .select_invocation_member(name, descriptor)
        .ok_or(InvocationError::AbstractMethod)?;
    pipeline.invoke(LambdaMethodCall {
        member,
        implementation,
        adaptations: plan.body().adaptations(),
        captures,
        arguments,
        resume,
    })
}

/// Loader-local class space for byte-free lambda definitions.
#[derive(Default)]
pub struct GeneratedLambdaClassSpace {
    classes: BTreeMap<(crate::ClassLoaderId, String), GeneratedLambdaClassEntry>,
}

struct GeneratedLambdaClassEntry {
    owner: Weak<ClassDefinition>,
    class: Arc<GeneratedLambdaClass>,
}

impl GeneratedLambdaClassSpace {
    /// Creates an empty generated-class registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs or returns the stable class for one exact linkage site.
    #[allow(clippy::too_many_arguments)]
    pub fn define(
        &mut self,
        cx: &Cx,
        heap: &mut JvmHeap,
        loader: &ClassLoader,
        owner: &Arc<ClassDefinition>,
        site: &SiteKey,
        factory_descriptor: &str,
        functional: &FunctionalInterface,
        plan: &LambdaBootstrapPlan,
    ) -> Result<Arc<GeneratedLambdaClass>, GeneratedLambdaClassError> {
        let fingerprint = lambda_site_fingerprint(site);
        let binary_name = format!(
            "{}$$Lambda${fingerprint:016x}",
            site.class.binary_name().replace('/', ".")
        );
        let key = (loader.id(), binary_name.clone());
        self.classes
            .retain(|_, entry| entry.owner.strong_count() != 0);
        if let Some(existing) = self.classes.get(&key) {
            return Ok(existing.class.clone());
        }

        let shape: ShapeRef = cx
            .factory()
            .opaque(Arc::new(AnyShape))
            .map_err(|error| GeneratedLambdaClassError::Metadata(error.to_string()))?;
        let mut parent_names = vec![functional.interface.clone()];
        parent_names.extend(plan.marker_interfaces.iter().cloned());
        if plan.serializable
            && !parent_names
                .iter()
                .any(|name| name == "java.io.Serializable")
        {
            parent_names.push("java.io.Serializable".into());
        }
        let mut seen_parents = BTreeSet::new();
        parent_names.retain(|name| seen_parents.insert(name.clone()));
        let parents = parent_names
            .iter()
            .map(|name| {
                let identity = generated_identity(loader.id(), name, stable_text_hash(name))?;
                Ok(DeclaredParent::unresolved(
                    identity,
                    Ref::Symbol(Symbol::new(name.clone())),
                ))
            })
            .collect::<Result<Vec<_>, GeneratedLambdaClassError>>()?;

        let mut members = vec![
            GeneratedLambdaMember {
                name: "<init>".into(),
                descriptor: factory_descriptor.into(),
                role: GeneratedLambdaMemberRole::FactoryConstructor,
            },
            GeneratedLambdaMember {
                name: functional.method_name.clone(),
                descriptor: plan.instantiated_method_type.clone(),
                role: GeneratedLambdaMemberRole::Sam,
            },
        ];
        members.extend(
            plan.bridges
                .iter()
                .cloned()
                .map(|descriptor| GeneratedLambdaMember {
                    name: functional.method_name.clone(),
                    descriptor,
                    role: GeneratedLambdaMemberRole::Bridge,
                }),
        );
        let projected_members = members
            .iter()
            .enumerate()
            .map(|(index, _member)| MemberShape {
                // The ordinal preserves duplicate name/descriptor bridge declarations
                // while leaving the exact JVM identity in the linker metadata.
                name: Symbol::new(format!("lambda-member-{index}")),
                shape: shape.clone(),
            })
            .collect();
        let descriptor = ClassDescriptor::new(ClassDescriptorInput {
            identity: generated_identity(loader.id(), &binary_name, fingerprint)?,
            parents,
            constructor_shape: shape.clone(),
            instance_shape: shape,
            members: projected_members,
            read_construction: None,
            metadata: vec![
                OpenMetadataEntry {
                    name: Symbol::new("jvm.generated-kind"),
                    value: cx
                        .factory()
                        .string("lambda".into())
                        .map_err(|error| GeneratedLambdaClassError::Metadata(error.to_string()))?,
                },
                OpenMetadataEntry {
                    name: Symbol::new("jvm.factory-descriptor"),
                    value: cx
                        .factory()
                        .string(factory_descriptor.into())
                        .map_err(|error| GeneratedLambdaClassError::Metadata(error.to_string()))?,
                },
            ],
        })
        .map_err(|error| GeneratedLambdaClassError::Metadata(error.to_string()))?;
        let mirror = heap
            .allocate(crate::JvmRole::ClassMirror)
            .map_err(|error| GeneratedLambdaClassError::Managed(format!("{error:?}")))?;
        let generated = Arc::new(GeneratedLambdaClass {
            binary_name,
            loader: loader.id(),
            mirror,
            descriptor,
            members,
            serializable: plan.serializable,
        });
        self.classes.insert(
            key,
            GeneratedLambdaClassEntry {
                owner: Arc::downgrade(owner),
                class: generated.clone(),
            },
        );
        Ok(generated)
    }

    /// Returns generated classes for one loader in stable binary-name order.
    pub fn browse(
        &self,
        loader: crate::ClassLoaderId,
        limit: usize,
    ) -> Vec<Arc<GeneratedLambdaClass>> {
        self.classes
            .range((loader, String::new())..)
            .take_while(|((found, _), _)| *found == loader)
            .take(limit)
            .filter(|(_, entry)| entry.owner.strong_count() != 0)
            .map(|(_, entry)| entry.class.clone())
            .collect()
    }

    /// Number of generated classes whose capturing class remains live.
    pub fn live_len(&mut self) -> usize {
        self.classes
            .retain(|_, entry| entry.owner.strong_count() != 0);
        self.classes.len()
    }
}

/// Java-permitted identity policy for a non-capturing lambda site.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum StatelessLambdaIdentity {
    /// Allocate on every factory call. This is always valid Java behavior.
    #[default]
    Fresh,
    /// Reuse one instance. Java permits this only for a non-capturing site.
    PermittedSingleton,
}

/// One linked, loader-owned lambda factory.
pub struct ManagedLambdaFactory {
    class: Arc<GeneratedLambdaClass>,
    plan: JvmFunctionPlan,
    class_value: ClassRef,
    managed: ManagedHandle,
    identity: StatelessLambdaIdentity,
    singleton: Option<(Arc<FunctionInstance<JvmFunctionPolicyBody>>, ManagedHandle)>,
}

impl ManagedLambdaFactory {
    /// Managed factory node stored as the value of the site ephemeron.
    pub const fn managed(&self) -> ManagedHandle {
        self.managed
    }

    /// Generated class owned by this factory.
    pub fn generated_class(&self) -> &Arc<GeneratedLambdaClass> {
        &self.class
    }

    /// Allocates an instance with captures in exact frozen-plan order.
    pub fn instantiate(
        &mut self,
        heap: &mut JvmHeap,
        captures: Vec<CapturedBinding>,
    ) -> Result<ManagedLambdaInstance, LambdaFactoryError> {
        if captures.len() != self.plan.neutral().captures().len() {
            return Err(LambdaFactoryError::CaptureArity {
                expected: self.plan.neutral().captures().len(),
                actual: captures.len(),
            });
        }
        if captures.is_empty()
            && self.identity == StatelessLambdaIdentity::PermittedSingleton
            && let Some((function, managed)) = &self.singleton
        {
            let root = heap.root(*managed).map_err(LambdaFactoryError::managed)?;
            return Ok(ManagedLambdaInstance {
                function: function.clone(),
                managed: *managed,
                root,
            });
        }
        let function = Arc::new(
            FunctionInstance::new(
                self.plan.neutral().clone(),
                self.plan.body().clone(),
                captures,
                self.class_value.clone(),
                None,
                None,
            )
            .map_err(|error| LambdaFactoryError::Instance(error.to_string()))?,
        );
        let managed = heap
            .allocate(crate::JvmRole::Object)
            .map_err(LambdaFactoryError::managed)?;
        heap.strong(managed, crate::JvmEdge::Class, self.class.mirror())
            .map_err(LambdaFactoryError::graph)?;
        for capture in function.captures() {
            heap.strong(managed, crate::JvmEdge::Field, capture.managed())
                .map_err(LambdaFactoryError::graph)?;
        }
        if function.captures().is_empty()
            && self.identity == StatelessLambdaIdentity::PermittedSingleton
        {
            heap.strong(self.managed, crate::JvmEdge::Field, managed)
                .map_err(LambdaFactoryError::graph)?;
            self.singleton = Some((function.clone(), managed));
        }
        let root = heap.root(managed).map_err(LambdaFactoryError::managed)?;
        Ok(ManagedLambdaInstance {
            function,
            managed,
            root,
        })
    }
}

/// A rooted managed lease for one lambda object.
pub struct ManagedLambdaInstance {
    function: Arc<FunctionInstance<JvmFunctionPolicyBody>>,
    managed: ManagedHandle,
    root: RootedHandle,
}

/// Located refusal to manufacture a Java serialized-lambda replacement.
///
/// SIM does not invoke host Java serialization or serialize the Rust function
/// object. A replacement can be admitted only after the JVM language library
/// owns exact managed `SerializedLambda` data and an authorized, validating
/// read-resolution protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LambdaSerializationError {
    /// The bootstrap did not declare this generated class serializable.
    NotDeclared {
        /// Capturing loader identity.
        loader: crate::ClassLoaderId,
        /// Generated class identity.
        class: String,
        /// Managed lambda object on which replacement was requested.
        object: ManagedHandle,
    },
    /// Serialization was declared, but the exact managed replacement/read
    /// protocol is not present and no weaker host mechanism is permitted.
    ManagedReplacementUnavailable {
        /// Capturing loader identity.
        loader: crate::ClassLoaderId,
        /// Generated class identity.
        class: String,
        /// Managed lambda object on which replacement was requested.
        object: ManagedHandle,
    },
}

impl ManagedLambdaInstance {
    /// Neutral function object carrying the exact capture cells.
    pub fn function(&self) -> &Arc<FunctionInstance<JvmFunctionPolicyBody>> {
        &self.function
    }

    /// Managed JVM object identity.
    pub const fn managed(&self) -> ManagedHandle {
        self.managed
    }

    /// Refuses serialization until the exact managed replacement protocol exists.
    ///
    /// This is deliberately a located runtime failure instead of an opaque
    /// omission. In particular, this method never consults a host JVM, a host
    /// serializer, or the captured Rust [`FunctionInstance`].
    pub fn serialized_replacement(
        &self,
        class: &GeneratedLambdaClass,
    ) -> Result<std::convert::Infallible, LambdaSerializationError> {
        let location = || (class.loader(), class.binary_name().to_owned(), self.managed);
        if class.serializable() {
            let (loader, class, object) = location();
            Err(LambdaSerializationError::ManagedReplacementUnavailable {
                loader,
                class,
                object,
            })
        } else {
            let (loader, class, object) = location();
            Err(LambdaSerializationError::NotDeclared {
                loader,
                class,
                object,
            })
        }
    }

    /// Releases this explicit heap root.
    pub fn release(self, heap: &mut JvmHeap) -> Result<(), LambdaFactoryError> {
        heap.release_root(self.root)
            .map_err(LambdaFactoryError::managed)?;
        Ok(())
    }
}

struct LambdaFactoryEntry {
    owner: Weak<ClassDefinition>,
    factory: Arc<std::sync::Mutex<ManagedLambdaFactory>>,
}

/// Occurrence-keyed factory cache whose managed entries are owner ephemerons.
#[derive(Default)]
pub struct LambdaFactoryCache {
    entries: BTreeMap<SiteKey, LambdaFactoryEntry>,
}

impl LambdaFactoryCache {
    /// Returns the existing factory for a live site or installs one.
    #[allow(clippy::too_many_arguments)]
    pub fn link(
        &mut self,
        heap: &mut JvmHeap,
        cache: ManagedHandle,
        owner_handle: ManagedHandle,
        owner: &Arc<ClassDefinition>,
        site: SiteKey,
        class: Arc<GeneratedLambdaClass>,
        plan: JvmFunctionPlan,
        class_value: ClassRef,
        identity: StatelessLambdaIdentity,
    ) -> Result<Arc<std::sync::Mutex<ManagedLambdaFactory>>, LambdaFactoryError> {
        self.entries
            .retain(|_, entry| entry.owner.strong_count() != 0);
        if !plan.neutral().captures().is_empty()
            && identity == StatelessLambdaIdentity::PermittedSingleton
        {
            return Err(LambdaFactoryError::CapturingSingleton);
        }
        if let Some(entry) = self.entries.get(&site) {
            return Ok(entry.factory.clone());
        }
        let managed = heap
            .allocate(crate::JvmRole::Object)
            .map_err(LambdaFactoryError::managed)?;
        heap.strong(managed, crate::JvmEdge::Class, class.mirror())
            .map_err(LambdaFactoryError::graph)?;
        heap.ephemeron(cache, crate::JvmEdge::DerivedEntry, owner_handle, managed)
            .map_err(LambdaFactoryError::graph)?;
        let factory = Arc::new(std::sync::Mutex::new(ManagedLambdaFactory {
            class,
            plan,
            class_value,
            managed,
            identity,
            singleton: None,
        }));
        self.entries.insert(
            site,
            LambdaFactoryEntry {
                owner: Arc::downgrade(owner),
                factory: factory.clone(),
            },
        );
        Ok(factory)
    }

    /// Number of entries whose capturing class loader remains live.
    pub fn live_len(&mut self) -> usize {
        self.entries
            .retain(|_, entry| entry.owner.strong_count() != 0);
        self.entries.len()
    }
}

/// Failure to link a factory or allocate a lambda instance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LambdaFactoryError {
    /// Captures did not exactly fill the frozen neutral plan.
    CaptureArity {
        /// Frozen capture-slot count.
        expected: usize,
        /// Supplied capture-cell count.
        actual: usize,
    },
    /// Singleton reuse is forbidden for capturing lambdas.
    CapturingSingleton,
    /// Neutral function construction failed.
    Instance(String),
    /// Managed allocation or rooting failed.
    Managed(String),
    /// Managed edge construction failed.
    Graph(String),
}

impl LambdaFactoryError {
    fn managed(error: impl std::fmt::Debug) -> Self {
        Self::Managed(format!("{error:?}"))
    }

    fn graph(error: impl std::fmt::Debug) -> Self {
        Self::Graph(format!("{error:?}"))
    }
}

/// Failure to assemble a managed byte-free lambda class.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GeneratedLambdaClassError {
    /// Checked neutral metadata rejected the generated declaration.
    Metadata(String),
    /// Managed class-mirror allocation failed.
    Managed(String),
}

fn generated_identity(
    loader: crate::ClassLoaderId,
    name: &str,
    fingerprint: u64,
) -> Result<ClassIdentity, GeneratedLambdaClassError> {
    let folded = fingerprint ^ loader.0 ^ (loader.0.rotate_left(23));
    let raw = ((folded >> 32) as u32 ^ folded as u32).max(1);
    ClassIdentity::checked(ClassId(raw), Symbol::new(name.to_owned()))
        .map_err(|error| GeneratedLambdaClassError::Metadata(error.to_string()))
}

fn lambda_site_fingerprint(site: &SiteKey) -> u64 {
    let mut hash = stable_text_hash(site.class.binary_name());
    hash = stable_hash_bytes(hash, &site.class.content_key().to_le_bytes());
    hash = stable_hash_bytes(hash, site.method.name.as_bytes());
    hash = stable_hash_bytes(hash, site.method.descriptor.as_bytes());
    stable_hash_bytes(hash, &site.constant_pool_index.to_le_bytes())
}

fn stable_text_hash(value: &str) -> u64 {
    stable_hash_bytes(0xcbf29ce484222325, value.as_bytes())
}

fn stable_hash_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    hash
}

/// Fail-closed lambda bootstrap admission or payload error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LambdaBootstrapError {
    /// Bootstrap identity is absent from the shared registry.
    UnadmittedProtocol,
    /// Payload length, kind, flags, count, or descriptor is malformed.
    MalformedPayload(String),
    /// Implementation handle is not an invocable method handle.
    UnadmittedReferenceKind(u8),
}

/// Located evidence that a loaded interface has one Java single abstract method.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionalInterface {
    /// Interface named by the invokedynamic call-site return type.
    pub interface: String,
    /// Exact erased method name.
    pub method_name: String,
    /// Exact erased SAM descriptor.
    pub method_descriptor: String,
    /// Loaded interfaces consulted, in deterministic traversal order.
    pub lineage: Vec<String>,
}

/// Fail-closed functional-interface and metafactory type validation error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FunctionalInterfaceError {
    /// SIM-to-Java interop was refused before generated class construction.
    InteropRefused(String),
    /// A required class is absent from the caller's already-loaded view.
    MissingClass(String),
    /// The call-site return or marker type is not an interface.
    NotInterface(String),
    /// A marker interface is not accessible to the capturing linkage site.
    InaccessibleInterface(String),
    /// The bounded interface walk would consult more nodes than allowed.
    HierarchyBudgetExhausted {
        /// Maximum loaded interface nodes the caller admitted.
        limit: usize,
    },
    /// No abstract method remains after Java `Object` exclusions.
    NoAbstractMethod {
        /// Interface whose inherited declarations were inspected.
        interface: String,
    },
    /// More than one unrelated abstract method signature remains.
    MultipleAbstractMethods {
        /// Deterministically ordered incompatible method identities.
        methods: Vec<String>,
    },
    /// A method descriptor or the invoked type is structurally invalid.
    InvalidDescriptor(String),
    /// Bootstrap and discovered SAM types disagree.
    SamTypeMismatch {
        /// Descriptor discovered from the interface hierarchy.
        discovered: String,
        /// Descriptor supplied by the bootstrap payload.
        supplied: String,
    },
    /// The instantiated or implementation method cannot implement the SAM.
    IncompatibleMethodType {
        /// Type-bearing bootstrap input that failed adaptation.
        role: &'static str,
        /// Descriptor rejected for that role.
        descriptor: String,
    },
}

/// Discovers the Java single abstract method through loaded interface inheritance.
///
/// The walk is deliberately loader-local and bounded. Static, private, default,
/// and public `java.lang.Object` methods do not contribute a SAM candidate.
pub fn discover_functional_interface(
    classes: &BTreeMap<String, Arc<ClassDefinition>>,
    interface: &str,
    node_limit: usize,
) -> Result<FunctionalInterface, FunctionalInterfaceError> {
    let mut pending = vec![interface.to_owned()];
    let mut visited = BTreeSet::new();
    let mut lineage = Vec::new();
    let mut methods: BTreeMap<(String, String), String> = BTreeMap::new();
    while let Some(name) = pending.pop() {
        if visited.contains(&name) {
            continue;
        }
        if visited.len() == node_limit {
            return Err(FunctionalInterfaceError::HierarchyBudgetExhausted { limit: node_limit });
        }
        let class = classes
            .get(&name)
            .ok_or_else(|| FunctionalInterfaceError::MissingClass(name.clone()))?;
        if class.metadata().access_flags() & 0x0200 == 0 {
            return Err(FunctionalInterfaceError::NotInterface(name));
        }
        visited.insert(name.clone());
        lineage.push(name);
        for method in class.metadata().members() {
            if method.kind() != crate::JavaMemberKind::Method
                || method.is_static()
                || !method.is_abstract()
                || method.access_flags() & 0x0002 != 0
                || object_method(method.name(), method.descriptor())
            {
                continue;
            }
            let close = method.descriptor().find(')').ok_or_else(|| {
                FunctionalInterfaceError::InvalidDescriptor(method.descriptor().into())
            })?;
            let key = (
                method.name().to_owned(),
                method.descriptor()[..=close].to_owned(),
            );
            methods
                .entry(key)
                .or_insert_with(|| method.descriptor().to_owned());
        }
        for parent in class.metadata().resolution().direct_parents().iter().rev() {
            if parent != "java.lang.Object" {
                pending.push(parent.clone());
            }
        }
    }
    if methods.is_empty() {
        return Err(FunctionalInterfaceError::NoAbstractMethod {
            interface: interface.into(),
        });
    }
    if methods.len() != 1 {
        return Err(FunctionalInterfaceError::MultipleAbstractMethods {
            methods: methods
                .into_iter()
                .map(|((name, _), descriptor)| format!("{name}{descriptor}"))
                .collect(),
        });
    }
    let ((method_name, _), method_descriptor) = methods.into_iter().next().unwrap();
    Ok(FunctionalInterface {
        interface: interface.into(),
        method_name,
        method_descriptor,
        lineage,
    })
}

/// Validates the located SAM and all metafactory type-bearing inputs.
pub fn validate_functional_interface(
    classes: &BTreeMap<String, Arc<ClassDefinition>>,
    capturing_class: &str,
    invoked_type: &str,
    plan: &LambdaBootstrapPlan,
    implementation_descriptor: &str,
    node_limit: usize,
) -> Result<FunctionalInterface, FunctionalInterfaceError> {
    let (captures, invoked_return) = split_method_descriptor(invoked_type)?;
    let interface = invoked_return
        .strip_prefix('L')
        .and_then(|v| v.strip_suffix(';'))
        .ok_or_else(|| FunctionalInterfaceError::InvalidDescriptor(invoked_type.into()))?
        .replace('/', ".");
    let functional = discover_functional_interface(classes, &interface, node_limit)?;
    if functional.method_descriptor != plan.sam_method_type {
        return Err(FunctionalInterfaceError::SamTypeMismatch {
            discovered: functional.method_descriptor.clone(),
            supplied: plan.sam_method_type.clone(),
        });
    }
    let (sam_args, sam_return) = split_method_descriptor(&plan.sam_method_type)?;
    let (instantiated_args, instantiated_return) =
        split_method_descriptor(&plan.instantiated_method_type)?;
    if sam_args.len() != instantiated_args.len()
        || !types_adapt(&instantiated_args, &sam_args)
        || !return_adapts(&instantiated_return, &sam_return)
    {
        return Err(FunctionalInterfaceError::IncompatibleMethodType {
            role: "instantiated",
            descriptor: plan.instantiated_method_type.clone(),
        });
    }
    let (implementation_args, implementation_return) =
        split_method_descriptor(implementation_descriptor)?;
    let mut supplied = captures;
    supplied.extend(instantiated_args.iter().cloned());
    if !types_adapt(&supplied, &implementation_args)
        || !return_adapts(&implementation_return, &instantiated_return)
    {
        return Err(FunctionalInterfaceError::IncompatibleMethodType {
            role: "implementation",
            descriptor: implementation_descriptor.into(),
        });
    }
    for marker in &plan.marker_interfaces {
        let marker = classes
            .get(marker)
            .ok_or_else(|| FunctionalInterfaceError::MissingClass(marker.clone()))?;
        if marker.metadata().access_flags() & 0x0200 == 0 {
            return Err(FunctionalInterfaceError::NotInterface(
                marker.metadata().resolution().binary_name().into(),
            ));
        }
        let marker_name = marker.metadata().resolution().binary_name();
        if marker.metadata().access_flags() & 0x0001 == 0
            && binary_package(marker_name) != binary_package(capturing_class)
        {
            return Err(FunctionalInterfaceError::InaccessibleInterface(
                marker_name.into(),
            ));
        }
    }
    for bridge in &plan.bridges {
        let (args, result) = split_method_descriptor(bridge)?;
        if args.len() != sam_args.len()
            || !types_adapt(&instantiated_args, &args)
            || !return_adapts(&instantiated_return, &result)
        {
            return Err(FunctionalInterfaceError::IncompatibleMethodType {
                role: "bridge",
                descriptor: bridge.clone(),
            });
        }
    }
    Ok(functional)
}

fn binary_package(name: &str) -> &str {
    name.rsplit_once(['.', '/'])
        .map_or("", |(package, _)| package)
}

fn object_method(name: &str, descriptor: &str) -> bool {
    matches!(
        (name, descriptor),
        ("equals", "(Ljava/lang/Object;)Z")
            | ("hashCode", "()I")
            | ("toString", "()Ljava/lang/String;")
    )
}

fn split_method_descriptor(value: &str) -> Result<(Vec<String>, String), FunctionalInterfaceError> {
    if !valid_method_descriptor(value) {
        return Err(FunctionalInterfaceError::InvalidDescriptor(value.into()));
    }
    let bytes = value.as_bytes();
    let mut cursor = 1;
    let mut args = Vec::new();
    while bytes[cursor] != b')' {
        let start = cursor;
        let _ = parse_descriptor_type(bytes, &mut cursor, false);
        args.push(value[start..cursor].to_owned());
    }
    cursor += 1;
    Ok((args, value[cursor..].to_owned()))
}

fn types_adapt(actual: &[String], expected: &[String]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(a, e)| a == e || (is_reference(a) && is_reference(e)))
}

fn return_adapts(actual: &str, expected: &str) -> bool {
    expected == "V" || actual == expected || (is_reference(actual) && is_reference(expected))
}

fn is_reference(value: &str) -> bool {
    value.starts_with('L') || value.starts_with('[')
}

/// Decodes and validates a lambda bootstrap before any generated class or instance is allocated.
pub fn decode_lambda_bootstrap(
    owner: &str,
    name: &str,
    descriptor: &str,
    arguments: &[ResolvedBootstrapArgument],
) -> Result<LambdaBootstrapPlan, LambdaBootstrapError> {
    let protocol = LAMBDA_BOOTSTRAP_REGISTRY
        .protocols
        .iter()
        .find(|protocol| {
            protocol.owner == owner && protocol.name == name && protocol.descriptor == descriptor
        })
        .ok_or(LambdaBootstrapError::UnadmittedProtocol)?;
    let method_type = |index: usize| match arguments.get(index) {
        Some(ResolvedBootstrapArgument::MethodType(value)) if valid_method_descriptor(value) => {
            Ok(value.clone())
        }
        _ => Err(LambdaBootstrapError::MalformedPayload(format!(
            "argument {index} must be a valid MethodType"
        ))),
    };
    let sam_method_type = method_type(0)?;
    let implementation_reference_kind = match arguments.get(1) {
        Some(ResolvedBootstrapArgument::MethodHandle { reference_kind }) => {
            if LAMBDA_BOOTSTRAP_REGISTRY
                .reference_kinds
                .contains(&i64::from(*reference_kind))
            {
                *reference_kind
            } else {
                return Err(LambdaBootstrapError::UnadmittedReferenceKind(
                    *reference_kind,
                ));
            }
        }
        _ => {
            return Err(LambdaBootstrapError::MalformedPayload(
                "argument 1 must be a MethodHandle".into(),
            ));
        }
    };
    let instantiated_method_type = method_type(2)?;
    let mut marker_interfaces = Vec::new();
    let mut bridges = Vec::new();
    let mut serializable = false;
    match protocol.tail {
        LambdaProtocolTail::None if arguments.len() != 3 => {
            return Err(LambdaBootstrapError::MalformedPayload(
                "metafactory requires exactly 3 arguments".into(),
            ));
        }
        LambdaProtocolTail::None => {}
        LambdaProtocolTail::FlagGoverned => {
            let flags = match arguments.get(3) {
                Some(ResolvedBootstrapArgument::Integer(flags)) => {
                    let unknown =
                        *flags as u32 & !(LAMBDA_BOOTSTRAP_REGISTRY.admitted_flags_mask as u32);
                    if unknown != 0 {
                        return Err(LambdaBootstrapError::MalformedPayload(format!(
                            "unknown altMetafactory flag bit {}",
                            unknown.trailing_zeros()
                        )));
                    }
                    *flags
                }
                None | Some(_) => {
                    return Err(LambdaBootstrapError::MalformedPayload(
                        "altMetafactory argument 3 must be integer flags".into(),
                    ));
                }
            };
            serializable = flags & 1 != 0;
            let mut cursor = 4;
            if flags & 2 != 0 {
                let count = payload_count(arguments, &mut cursor, "marker")?;
                for _ in 0..count {
                    match arguments.get(cursor) {
                        Some(ResolvedBootstrapArgument::Class(name)) if !name.is_empty() => {
                            if marker_interfaces.iter().any(|marker| marker == name) {
                                return Err(LambdaBootstrapError::MalformedPayload(format!(
                                    "duplicate marker interface {name}"
                                )));
                            }
                            marker_interfaces.push(name.clone())
                        }
                        _ => {
                            return Err(LambdaBootstrapError::MalformedPayload(
                                "marker must be a class".into(),
                            ));
                        }
                    }
                    cursor += 1;
                }
            }
            if flags & 4 != 0 {
                let count = payload_count(arguments, &mut cursor, "bridge")?;
                for _ in 0..count {
                    let bridge = method_type(cursor)?;
                    if bridge == sam_method_type {
                        return Err(LambdaBootstrapError::MalformedPayload(format!(
                            "bridge {bridge} conflicts with the SAM method"
                        )));
                    }
                    if bridges.iter().any(|existing| existing == &bridge) {
                        return Err(LambdaBootstrapError::MalformedPayload(format!(
                            "duplicate bridge {bridge}"
                        )));
                    }
                    bridges.push(bridge);
                    cursor += 1;
                }
            }
            if cursor != arguments.len() {
                return Err(LambdaBootstrapError::MalformedPayload(
                    "trailing altMetafactory arguments".into(),
                ));
            }
        }
    }
    Ok(LambdaBootstrapPlan {
        sam_method_type,
        implementation_reference_kind,
        instantiated_method_type,
        marker_interfaces,
        bridges,
        serializable,
    })
}

fn payload_count(
    arguments: &[ResolvedBootstrapArgument],
    cursor: &mut usize,
    label: &str,
) -> Result<usize, LambdaBootstrapError> {
    let count = match arguments.get(*cursor) {
        Some(ResolvedBootstrapArgument::Integer(value)) if *value >= 0 => {
            usize::try_from(*value).unwrap()
        }
        _ => {
            return Err(LambdaBootstrapError::MalformedPayload(format!(
                "{label} count is missing or negative"
            )));
        }
    };
    *cursor += 1;
    if count > arguments.len().saturating_sub(*cursor) {
        return Err(LambdaBootstrapError::MalformedPayload(format!(
            "{label} payload is truncated"
        )));
    }
    Ok(count)
}

fn valid_method_descriptor(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.first() != Some(&b'(') {
        return false;
    }
    let mut cursor = 1;
    while bytes.get(cursor) != Some(&b')') {
        if !parse_descriptor_type(bytes, &mut cursor, false) {
            return false;
        }
    }
    cursor += 1;
    if bytes.get(cursor) == Some(&b'V') {
        cursor += 1;
    } else if !parse_descriptor_type(bytes, &mut cursor, false) {
        return false;
    }
    cursor == bytes.len()
}

fn parse_descriptor_type(bytes: &[u8], cursor: &mut usize, in_array: bool) -> bool {
    match bytes.get(*cursor).copied() {
        Some(b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z') => {
            *cursor += 1;
            true
        }
        Some(b'L') => {
            *cursor += 1;
            let start = *cursor;
            while !matches!(bytes.get(*cursor), None | Some(b';')) {
                *cursor += 1;
            }
            if *cursor == start || bytes.get(*cursor) != Some(&b';') {
                return false;
            }
            *cursor += 1;
            true
        }
        Some(b'[') if !in_array => {
            while bytes.get(*cursor) == Some(&b'[') {
                *cursor += 1;
            }
            parse_descriptor_type(bytes, cursor, true)
        }
        _ => false,
    }
}

/// Identity of the method enclosing a bootstrap occurrence.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct MethodIdentity {
    /// JVM method name.
    pub name: String,
    /// JVM method descriptor.
    pub descriptor: String,
}

/// One raw constant-pool bootstrap argument, before protocol interpretation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum BootstrapArgument {
    /// Constant-pool index of a loadable constant.
    Constant(u16),
    /// Exact integer payload.
    Integer(i32),
    /// Exact long payload.
    Long(i64),
    /// Exact IEEE-754 single payload.
    FloatBits(u32),
    /// Exact IEEE-754 double payload.
    DoubleBits(u64),
    /// Exact modified-UTF-8-decoded string code units.
    String(Box<[u16]>),
}

/// Raw decoded `BootstrapMethods` record retained without protocol assumptions.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct BootstrapMethod {
    /// Constant-pool index of the bootstrap method handle.
    pub method_handle: u16,
    /// Bootstrap arguments in classfile order.
    pub arguments: Box<[BootstrapArgument]>,
}

/// Immutable identity of one bootstrap instruction occurrence.
///
/// `class` carries loader, binary-name, and classfile-content identity. The
/// constant-pool index deliberately remains part of the key even when every
/// decoded bootstrap argument is byte-for-byte identical to another site.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct SiteKey {
    /// Exact defining class, including loader and classfile content identity.
    pub class: ClassDefinitionId,
    /// Method containing the instruction occurrence.
    pub method: MethodIdentity,
    /// Constant-pool index named by this instruction occurrence.
    pub constant_pool_index: u16,
    /// Raw decoded bootstrap record selected by the dynamic constant.
    pub bootstrap: BootstrapMethod,
}

/// Typed, cacheable bootstrap linkage failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkageFailure {
    /// A referenced constant-pool entry is absent or has the wrong kind.
    InvalidConstantPoolEntry(u16),
    /// The bootstrap protocol is not admitted by the installed linker.
    UnsupportedBootstrap {
        /// Refused bootstrap owner.
        owner: String,
        /// Refused bootstrap member name.
        name: String,
    },
    /// The dynamic invocation descriptor is malformed.
    InvalidDescriptor(String),
    /// Bootstrap execution failed with a stable JVM linkage condition.
    Bootstrap(String),
}

/// Revision-bound state of one linkage occurrence.
#[derive(Debug, Eq, PartialEq)]
pub enum LinkageState<T> {
    /// The site has not yet been linked at this revision.
    Unlinked,
    /// Successful immutable linkage product.
    Linked(Arc<T>),
    /// Stable typed failure produced while linking.
    Failed(LinkageFailure),
}

impl<T> Clone for LinkageState<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Unlinked => Self::Unlinked,
            Self::Linked(value) => Self::Linked(Arc::clone(value)),
            Self::Failed(error) => Self::Failed(error.clone()),
        }
    }
}

#[derive(Clone, Debug)]
struct CacheEntry<T> {
    revision: ClassSpaceRevision,
    state: LinkageState<T>,
}

/// Per-occurrence cache whose successes and failures expire together on a
/// class-space revision change.
#[derive(Clone, Debug, Default)]
pub struct LinkageCache<T> {
    entries: BTreeMap<SiteKey, CacheEntry<T>>,
}

impl<T> LinkageCache<T> {
    /// Creates an empty linkage cache.
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Returns the current state, treating an entry from another loader
    /// revision as unlinked rather than exposing stale linkage.
    pub fn state(&self, key: &SiteKey, revision: ClassSpaceRevision) -> LinkageState<T> {
        self.entries
            .get(key)
            .filter(|entry| entry.revision == revision)
            .map_or(LinkageState::Unlinked, |entry| entry.state.clone())
    }

    /// Links once per exact site and revision, caching typed failures as well as
    /// successful products. A stale entry is replaced only after `link` runs.
    pub fn resolve<F>(
        &mut self,
        key: SiteKey,
        revision: ClassSpaceRevision,
        link: F,
    ) -> Result<Arc<T>, LinkageFailure>
    where
        F: FnOnce() -> Result<T, LinkageFailure>,
    {
        if let Some(entry) = self
            .entries
            .get(&key)
            .filter(|entry| entry.revision == revision)
        {
            return match &entry.state {
                LinkageState::Linked(value) => Ok(Arc::clone(value)),
                LinkageState::Failed(error) => Err(error.clone()),
                LinkageState::Unlinked => unreachable!("unlinked states are not stored"),
            };
        }
        let state = match link() {
            Ok(value) => LinkageState::Linked(Arc::new(value)),
            Err(error) => LinkageState::Failed(error),
        };
        self.entries.insert(
            key,
            CacheEntry {
                revision,
                state: state.clone(),
            },
        );
        match state {
            LinkageState::Linked(value) => Ok(value),
            LinkageState::Failed(error) => Err(error),
            LinkageState::Unlinked => unreachable!("unlinked states are not stored"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClassLoader, JavaClassMetadata, JvmRole, resolution::SymbolicConstant};
    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
    use sim_lib_binding::BindingCell;
    use sim_lib_function::{CallMode, CaptureDescriptor, ParameterDescriptor, ParameterKind};
    use sim_lib_gc_tracing::CollectionLimits;

    fn neutral_plan(captures: usize, parameters: usize) -> FunctionPlan {
        FunctionPlan::new(
            sim_kernel::Symbol::new("jvm:test"),
            (0..parameters)
                .map(|index| {
                    ParameterDescriptor::new(
                        sim_kernel::Symbol::new(format!("p{index}")),
                        ParameterKind::Required,
                        CallMode::POSITIONAL,
                        None,
                    )
                })
                .collect(),
            (0..captures)
                .map(|index| {
                    CaptureDescriptor::new(sim_kernel::Symbol::new(format!("c{index}")), None)
                })
                .collect(),
            None,
        )
        .unwrap()
    }

    #[test]
    fn java_lambda_is_an_ordinary_callable_and_sim_adapter_refuses_before_generation() {
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        cx.grant(crate::jvm_invoke_capability());
        let shape = cx.factory().opaque(Arc::new(AnyShape)).unwrap();
        let lambda = JavaLambdaCallable::new(vec![shape], None, |_cx, mut args| {
            JavaLambdaCallOutcome::Returned(args.remove(0))
        });
        let expected = cx.factory().string("shared function".into()).unwrap();
        let actual = lambda
            .call(&mut cx, Args::new(vec![expected.clone()]))
            .unwrap();
        assert_eq!(
            actual.object().display(&mut cx).unwrap(),
            expected.object().display(&mut cx).unwrap()
        );

        let value = cx.factory().opaque(Arc::new(lambda)).unwrap();
        let generated = std::sync::atomic::AtomicBool::new(false);
        let refused = adapt_sim_callable_as_functional_interface(&mut cx, value, || {
            generated.store(true, std::sync::atomic::Ordering::SeqCst);
            unreachable!("generation must follow capability admission")
        });
        assert!(matches!(
            refused,
            Err(FunctionalInterfaceError::InteropRefused(_))
        ));
        assert!(!generated.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn compiles_capture_receiver_parameter_and_return_policy_over_neutral_plan() {
        let compiled = compile_jvm_function_plan(
            neutral_plan(2, 2),
            "(Ljava/lang/String;I)Lexample/Fn;",
            "(Ljava/lang/Integer;I)Ljava/lang/Integer;",
            "(JILjava/lang/Object;)I",
            DirectReceiver::Bound,
            Some("Ljava/lang/Object;"),
        )
        .unwrap();
        assert_eq!(compiled.neutral().captures().len(), 2);
        assert_eq!(
            compiled.body().adaptations(),
            [
                LocatedJvmAdaptation {
                    point: AdaptationPoint::Receiver,
                    adaptation: JvmAdaptation::ReferenceCast {
                        from: "Ljava/lang/String;".into(),
                        to: "Ljava/lang/Object;".into(),
                    },
                },
                LocatedJvmAdaptation {
                    point: AdaptationPoint::Capture(1),
                    adaptation: JvmAdaptation::PrimitiveWiden { from: 'I', to: 'J' },
                },
                LocatedJvmAdaptation {
                    point: AdaptationPoint::Parameter(0),
                    adaptation: JvmAdaptation::Unbox {
                        reference: "Ljava/lang/Integer;".into(),
                        primitive: 'I',
                    },
                },
                LocatedJvmAdaptation {
                    point: AdaptationPoint::Parameter(1),
                    adaptation: JvmAdaptation::Box {
                        primitive: 'I',
                        reference: "Ljava/lang/Object;".into(),
                    },
                },
                LocatedJvmAdaptation {
                    point: AdaptationPoint::Return,
                    adaptation: JvmAdaptation::Box {
                        primitive: 'I',
                        reference: "Ljava/lang/Integer;".into(),
                    },
                },
            ]
        );
    }

    #[test]
    fn bad_adaptation_stage_precedes_factory_allocation_and_invocation() {
        assert_eq!(
            compile_jvm_function_plan(
                neutral_plan(0, 1),
                "()Lexample/Fn;",
                "(J)I",
                "(I)I",
                DirectReceiver::None,
                None,
            ),
            Err(JvmAdaptationError::UnsupportedConversion {
                point: AdaptationPoint::Parameter(0),
                from: "J".into(),
                to: "I".into(),
            })
        );
        assert_eq!(
            compile_jvm_function_plan(
                neutral_plan(0, 0),
                "()Lexample/Fn;",
                "()I",
                "()V",
                DirectReceiver::None,
                None,
            ),
            Err(JvmAdaptationError::VoidToValue {
                point: AdaptationPoint::Return,
                required: "I".into(),
            })
        );
    }

    #[test]
    fn neutral_function_sources_contain_no_java_descriptor_vocabulary() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../sim-lib-function/src");
        let forbidden = [
            "java/lang",
            "LambdaMetafactory",
            "MethodType",
            "invokedynamic",
        ];
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|extension| extension == "rs") {
                let source = std::fs::read_to_string(&path).unwrap();
                for vocabulary in forbidden {
                    assert!(
                        !source.contains(vocabulary),
                        "neutral source {} contains JVM vocabulary {vocabulary}",
                        path.display()
                    );
                }
            }
        }
    }

    #[test]
    fn generated_lambda_class_is_stable_browsable_and_shape_checked_without_bytes() {
        let (site, loader) = fixture();
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let mut heap = JvmHeap::new(
            32,
            CollectionLimits {
                objects: 32,
                edges: 32,
                stack: 32,
                work: 128,
                clears: 32,
                finalizers: 0,
            },
        )
        .unwrap();
        let functional = FunctionalInterface {
            interface: "example.Function".into(),
            method_name: "apply".into(),
            method_descriptor: "(Ljava/lang/Object;)Ljava/lang/Object;".into(),
            lineage: vec!["example.Function".into()],
        };
        let plan = LambdaBootstrapPlan {
            sam_method_type: functional.method_descriptor.clone(),
            implementation_reference_kind: 6,
            instantiated_method_type: "(Ljava/lang/String;)Ljava/lang/String;".into(),
            marker_interfaces: vec!["example.Marker".into()],
            bridges: vec!["(Ljava/lang/CharSequence;)Ljava/lang/Object;".into()],
            serializable: true,
        };
        let owner = ClassDefinition::test(
            loader.id(),
            site.class.binary_name(),
            site.class.content_key(),
            JavaClassMetadata::test_identity(&cx, site.class.binary_name(), &[]),
            BTreeMap::new(),
        );
        let mut classes = GeneratedLambdaClassSpace::new();
        let first = classes
            .define(
                &cx,
                &mut heap,
                &loader,
                &owner,
                &site,
                "(I)Lexample/Function;",
                &functional,
                &plan,
            )
            .unwrap();
        let repeated = classes
            .define(
                &cx,
                &mut heap,
                &loader,
                &owner,
                &site,
                "(I)Lexample/Function;",
                &functional,
                &plan,
            )
            .unwrap();

        assert!(Arc::ptr_eq(&first, &repeated));
        assert_eq!(classes.browse(loader.id(), 8).len(), 1);
        assert_eq!(first.members().len(), 3);
        assert_eq!(first.members()[2].role(), GeneratedLambdaMemberRole::Bridge);
        let sam = first
            .select_invocation_member("apply", "(Ljava/lang/String;)Ljava/lang/String;")
            .unwrap();
        let bridge = first
            .select_invocation_member("apply", "(Ljava/lang/CharSequence;)Ljava/lang/Object;")
            .unwrap();
        assert_eq!(sam.role(), GeneratedLambdaMemberRole::Sam);
        assert_eq!(bridge.role(), GeneratedLambdaMemberRole::Bridge);
        assert!(
            first
                .select_invocation_member("apply", "(Ljava/lang/Object;)Ljava/lang/Object;")
                .is_none(),
            "selection must not fall back across erasures"
        );
        assert_eq!(
            first
                .descriptor()
                .parents()
                .iter()
                .map(|parent| parent.identity().symbol().name.as_ref())
                .collect::<Vec<_>>(),
            ["example.Function", "example.Marker", "java.io.Serializable"]
        );
        assert!(
            first
                .class_value(&cx, 16, 64)
                .unwrap()
                .object()
                .as_class()
                .is_some()
        );
        let sample = cx.factory().string("lambda instance".into()).unwrap();
        let checked = first
            .descriptor()
            .instance_shape()
            .object()
            .as_shape()
            .unwrap()
            .check_value(&mut cx, sample)
            .unwrap();
        assert!(checked.accepted);

        let source = include_str!("linker.rs");
        assert!(!source.contains(concat!("0xCAFE", "BABE")));
        assert!(!source.contains(concat!("define_", "bytes(")));
        assert!(!source.contains(concat!("Class", "Shell")));
    }

    #[test]
    fn loader_collection_stage_collects_a_captured_lambda_enclosing_object_cycle() {
        let (site, loader) = fixture();
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let owner = ClassDefinition::test(
            loader.id(),
            site.class.binary_name(),
            site.class.content_key(),
            JavaClassMetadata::test_identity(&cx, site.class.binary_name(), &[]),
            BTreeMap::new(),
        );
        let mut heap = JvmHeap::new(
            32,
            CollectionLimits {
                objects: 32,
                edges: 64,
                stack: 32,
                work: 256,
                clears: 32,
                finalizers: 0,
            },
        )
        .unwrap();
        let loader_node = heap.allocate(JvmRole::Loader).unwrap();
        let loader_root = heap.root(loader_node).unwrap();
        let owner_node = heap.allocate(JvmRole::ClassMirror).unwrap();
        heap.strong(loader_node, crate::JvmEdge::DefinedClass, owner_node)
            .unwrap();
        heap.strong(owner_node, crate::JvmEdge::DefiningLoader, loader_node)
            .unwrap();
        let cache_node = heap.allocate(JvmRole::Cache).unwrap();
        let _cache_root = heap.root(cache_node).unwrap();
        let capture_node = heap.allocate(JvmRole::Object).unwrap();
        let capture_root = heap.root(capture_node).unwrap();

        let functional = FunctionalInterface {
            interface: "example.Function".into(),
            method_name: "apply".into(),
            method_descriptor: "()Ljava/lang/Object;".into(),
            lineage: vec!["example.Function".into()],
        };
        let bootstrap = LambdaBootstrapPlan {
            sam_method_type: functional.method_descriptor.clone(),
            implementation_reference_kind: 6,
            instantiated_method_type: functional.method_descriptor.clone(),
            marker_interfaces: vec![],
            bridges: vec![],
            serializable: true,
        };
        let mut classes = GeneratedLambdaClassSpace::new();
        let generated = classes
            .define(
                &cx,
                &mut heap,
                &loader,
                &owner,
                &site,
                "(Ljava/lang/Object;)Lexample/Function;",
                &functional,
                &bootstrap,
            )
            .unwrap();
        let class_value = generated.class_value(&cx, 16, 64).unwrap();
        let plan = JvmFunctionPlan {
            neutral: neutral_plan(1, 0),
            body: JvmFunctionPolicyBody {
                adaptations: Box::new([]),
            },
        };
        let mut factories = LambdaFactoryCache::default();
        assert!(matches!(
            factories.link(
                &mut heap,
                cache_node,
                owner_node,
                &owner,
                site.clone(),
                generated.clone(),
                plan.clone(),
                class_value.clone(),
                StatelessLambdaIdentity::PermittedSingleton,
            ),
            Err(LambdaFactoryError::CapturingSingleton)
        ));
        let first_factory = factories
            .link(
                &mut heap,
                cache_node,
                owner_node,
                &owner,
                site.clone(),
                generated.clone(),
                plan.clone(),
                class_value.clone(),
                StatelessLambdaIdentity::Fresh,
            )
            .unwrap();
        let repeated_factory = factories
            .link(
                &mut heap,
                cache_node,
                owner_node,
                &owner,
                site.clone(),
                generated.clone(),
                plan,
                class_value,
                StatelessLambdaIdentity::Fresh,
            )
            .unwrap()
            .clone();
        assert!(Arc::ptr_eq(&first_factory, &repeated_factory));
        let factory_node = first_factory.lock().unwrap().managed();

        let captured_value = cx.factory().string("captured".into()).unwrap();
        let binding = || {
            CapturedBinding::new(
                BindingCell::initialized(Symbol::new("c0"), captured_value.clone()),
                capture_node,
            )
        };
        let first_instance = first_factory
            .lock()
            .unwrap()
            .instantiate(&mut heap, vec![binding()])
            .unwrap();
        heap.strong(
            capture_node,
            crate::JvmEdge::Field,
            first_instance.managed(),
        )
        .unwrap();
        let second_instance = first_factory
            .lock()
            .unwrap()
            .instantiate(&mut heap, vec![binding()])
            .unwrap();
        assert_ne!(first_instance.managed(), second_instance.managed());
        assert_eq!(
            first_instance.function().captures()[0]
                .cell()
                .get()
                .unwrap(),
            captured_value
        );
        assert_eq!(
            first_instance.serialized_replacement(&generated),
            Err(LambdaSerializationError::ManagedReplacementUnavailable {
                loader: loader.id(),
                class: generated.binary_name().to_owned(),
                object: first_instance.managed(),
            })
        );
        let source = include_str!("linker.rs");
        assert!(!source.contains(concat!("Object", "OutputStream")));
        assert!(!source.contains(concat!("bincode", "::serialize")));
        let first_instance_node = first_instance.managed();
        first_instance.release(&mut heap).unwrap();
        second_instance.release(&mut heap).unwrap();

        let mut stateless_site = site.clone();
        stateless_site.constant_pool_index += 1;
        let stateless = factories
            .link(
                &mut heap,
                cache_node,
                owner_node,
                &owner,
                stateless_site,
                generated.clone(),
                JvmFunctionPlan {
                    neutral: neutral_plan(0, 0),
                    body: JvmFunctionPolicyBody {
                        adaptations: Box::new([]),
                    },
                },
                generated.class_value(&cx, 16, 64).unwrap(),
                StatelessLambdaIdentity::PermittedSingleton,
            )
            .unwrap();
        let stateless_first = stateless
            .lock()
            .unwrap()
            .instantiate(&mut heap, vec![])
            .unwrap();
        let stateless_second = stateless
            .lock()
            .unwrap()
            .instantiate(&mut heap, vec![])
            .unwrap();
        assert_eq!(stateless_first.managed(), stateless_second.managed());
        stateless_first.release(&mut heap).unwrap();
        stateless_second.release(&mut heap).unwrap();
        heap.release_root(capture_root).unwrap();

        let weak_factory = Arc::downgrade(&first_factory);
        let weak_class = Arc::downgrade(&generated);
        drop(repeated_factory);
        drop(first_factory);
        drop(stateless);
        drop(generated);
        drop(owner);
        assert_eq!(factories.live_len(), 0);
        assert_eq!(classes.live_len(), 0);
        assert!(weak_factory.upgrade().is_none());
        assert!(weak_class.upgrade().is_none());
        heap.release_root(loader_root).unwrap();
        let receipt = heap.collect().unwrap();
        assert!(receipt.swept.contains(&factory_node.id()));
        assert!(receipt.swept.contains(&capture_node.id()));
        assert!(receipt.swept.contains(&first_instance_node.id()));
        assert_eq!(receipt.cleared_ephemerons.len(), 2);
        assert!(
            receipt
                .cleared_ephemerons
                .iter()
                .all(|(owner, _)| *owner == cache_node.id())
        );
    }

    fn fixture() -> (SiteKey, ClassLoader) {
        let loader = ClassLoader::new(4096);
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let class = crate::ClassDefinition::test(
            loader.id(),
            "Example",
            0x51_7e,
            JavaClassMetadata::test_identity(&cx, "Example", &[]),
            BTreeMap::new(),
        );
        (
            SiteKey {
                class: class.id().clone(),
                method: MethodIdentity {
                    name: "make".into(),
                    descriptor: "()V".into(),
                },
                constant_pool_index: 7,
                bootstrap: BootstrapMethod {
                    method_handle: 3,
                    arguments: vec![BootstrapArgument::Constant(11)].into_boxed_slice(),
                },
            },
            loader,
        )
    }

    fn interface(
        cx: &Cx,
        name: &str,
        parents: &[&str],
        methods: &[(&str, &str, u16)],
    ) -> Arc<ClassDefinition> {
        ClassDefinition::test(
            ClassLoader::new(32).id(),
            name,
            1,
            JavaClassMetadata::test_class(cx, name, parents, 0x0601, methods),
            BTreeMap::new(),
        )
    }

    #[test]
    fn invalid_sam_discovery_stage_rejects_object_method_before_generation() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let classes = BTreeMap::from([(
            "example.EqualsOnly".into(),
            interface(
                &cx,
                "example.EqualsOnly",
                &[],
                &[("equals", "(Ljava/lang/Object;)Z", 0x0401)],
            ),
        )]);
        assert_eq!(
            discover_functional_interface(&classes, "example.EqualsOnly", 1),
            Err(FunctionalInterfaceError::NoAbstractMethod {
                interface: "example.EqualsOnly".into()
            })
        );
    }

    #[test]
    fn unrelated_abstract_methods_are_both_named() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let classes = BTreeMap::from([(
            "example.Pair".into(),
            interface(
                &cx,
                "example.Pair",
                &[],
                &[("left", "()V", 0x0401), ("right", "(I)V", 0x0401)],
            ),
        )]);
        assert_eq!(
            discover_functional_interface(&classes, "example.Pair", 1),
            Err(FunctionalInterfaceError::MultipleAbstractMethods {
                methods: vec!["left()V".into(), "right(I)V".into()]
            })
        );
    }

    #[test]
    fn recursive_sam_discovery_work_limit_stage_precedes_generation() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let classes = BTreeMap::from([
            (
                "example.Child".into(),
                interface(&cx, "example.Child", &["example.Parent"], &[]),
            ),
            (
                "example.Parent".into(),
                interface(&cx, "example.Parent", &[], &[("apply", "(I)I", 0x0401)]),
            ),
        ]);
        assert_eq!(
            discover_functional_interface(&classes, "example.Child", 1),
            Err(FunctionalInterfaceError::HierarchyBudgetExhausted { limit: 1 })
        );
        let found = discover_functional_interface(&classes, "example.Child", 2).unwrap();
        assert_eq!(
            (found.method_name.as_str(), found.method_descriptor.as_str()),
            ("apply", "(I)I")
        );
        assert_eq!(found.lineage, ["example.Child", "example.Parent"]);
    }

    #[test]
    fn inaccessible_handle_and_invalid_sam_stage_precede_generation() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let classes = BTreeMap::from([
            (
                "example.Function".into(),
                interface(
                    &cx,
                    "example.Function",
                    &[],
                    &[("apply", "(Ljava/lang/Object;)Ljava/lang/Object;", 0x0401)],
                ),
            ),
            (
                "example.Marker".into(),
                interface(&cx, "example.Marker", &[], &[]),
            ),
        ]);
        let plan = LambdaBootstrapPlan {
            sam_method_type: "(Ljava/lang/Object;)Ljava/lang/Object;".into(),
            implementation_reference_kind: 6,
            instantiated_method_type: "(Ljava/lang/String;)Ljava/lang/String;".into(),
            marker_interfaces: vec!["example.Marker".into()],
            bridges: vec!["(Ljava/lang/CharSequence;)Ljava/lang/Object;".into()],
            serializable: false,
        };
        let found = validate_functional_interface(
            &classes,
            "example.Capturing",
            "(I)Lexample/Function;",
            &plan,
            "(ILjava/lang/String;)Ljava/lang/String;",
            2,
        )
        .unwrap();
        assert_eq!(found.method_name, "apply");

        let incompatible = validate_functional_interface(
            &classes,
            "example.Capturing",
            "(I)Lexample/Function;",
            &plan,
            "(JLjava/lang/String;)Ljava/lang/String;",
            2,
        )
        .unwrap_err();
        assert!(matches!(
            incompatible,
            FunctionalInterfaceError::IncompatibleMethodType {
                role: "implementation",
                ..
            }
        ));

        let inaccessible = ClassDefinition::test(
            ClassLoader::new(32).id(),
            "other.HiddenMarker",
            1,
            JavaClassMetadata::test_class(&cx, "other.HiddenMarker", &[], 0x0600, &[]),
            BTreeMap::new(),
        );
        let mut inaccessible_classes = classes.clone();
        inaccessible_classes.insert("other.HiddenMarker".into(), inaccessible);
        let mut inaccessible_plan = plan.clone();
        inaccessible_plan.marker_interfaces = vec!["other.HiddenMarker".into()];
        assert_eq!(
            validate_functional_interface(
                &inaccessible_classes,
                "example.Capturing",
                "(I)Lexample/Function;",
                &inaccessible_plan,
                "(ILjava/lang/String;)Ljava/lang/String;",
                2,
            ),
            Err(FunctionalInterfaceError::InaccessibleInterface(
                "other.HiddenMarker".into()
            ))
        );
    }

    #[test]
    fn malformed_linkage_stage_performs_no_class_or_heap_effect() {
        let (_site, loader) = fixture();
        let classes = GeneratedLambdaClassSpace::new();
        let heap = JvmHeap::new(
            8,
            CollectionLimits {
                objects: 8,
                edges: 8,
                stack: 8,
                work: 32,
                clears: 8,
                finalizers: 0,
            },
        )
        .unwrap();
        let protocol = &executor_admitted_lambda_protocols()[1];
        let refused = decode_lambda_bootstrap(
            protocol.owner,
            protocol.name,
            protocol.descriptor,
            &[
                ResolvedBootstrapArgument::MethodType("()V".into()),
                ResolvedBootstrapArgument::MethodHandle { reference_kind: 6 },
                ResolvedBootstrapArgument::MethodType("()V".into()),
                ResolvedBootstrapArgument::Integer(8),
            ],
        );

        assert_eq!(
            refused,
            Err(LambdaBootstrapError::MalformedPayload(
                "unknown altMetafactory flag bit 3".into()
            ))
        );
        assert!(classes.browse(loader.id(), 1).is_empty());
        assert_eq!(heap.live_len(), 0);
    }

    #[test]
    fn identical_lambdas_at_two_occurrences_are_distinct_sites() {
        let (first, loader) = fixture();
        let mut second = first.clone();
        second.constant_pool_index = 8;
        assert_ne!(first, second);
        let mut cache = LinkageCache::new();
        let revision = loader.revision();
        let first_value = cache
            .resolve(first, revision, || Ok::<_, LinkageFailure>("first"))
            .unwrap();
        let second_value = cache
            .resolve(second, revision, || Ok::<_, LinkageFailure>("second"))
            .unwrap();
        assert_eq!((*first_value, *second_value), ("first", "second"));
    }

    #[test]
    fn stale_proof_stage_relinks_after_revision_change() {
        let (key, loader) = fixture();
        let revision = loader.revision();
        loader.simulate_class_space_change();
        let next = loader.revision();
        let mut successes = LinkageCache::new();
        let original = successes
            .resolve(key.clone(), revision, || Ok::<_, LinkageFailure>(1))
            .unwrap();
        let relinked = successes
            .resolve(key.clone(), next, || Ok::<_, LinkageFailure>(2))
            .unwrap();
        assert_eq!((*original, *relinked), (1, 2));

        let mut failures = LinkageCache::<u8>::new();
        let stale = LinkageFailure::Bootstrap("stale".into());
        assert_eq!(
            failures.resolve(key.clone(), revision, || Err(stale.clone())),
            Err(stale)
        );
        assert_eq!(*failures.resolve(key, next, || Ok(9)).unwrap(), 9);
    }

    #[test]
    fn cached_failure_stage_performs_no_later_link_effect() {
        let (key, loader) = fixture();
        let revision = loader.revision();
        let mut cache = LinkageCache::<u8>::new();
        let attempts = std::sync::atomic::AtomicUsize::new(0);
        let failure = LinkageFailure::InvalidConstantPoolEntry(7);

        for _ in 0..2 {
            assert_eq!(
                cache.resolve(key.clone(), revision, || {
                    attempts.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Err(failure.clone())
                }),
                Err(failure.clone())
            );
        }
        assert_eq!(attempts.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(cache.state(&key, revision), LinkageState::Failed(failure));
    }

    #[test]
    fn allocation_limit_stage_precedes_graph_and_factory_effects() {
        let mut heap = JvmHeap::new(
            1,
            CollectionLimits {
                objects: 1,
                edges: 1,
                stack: 1,
                work: 1,
                clears: 1,
                finalizers: 0,
            },
        )
        .unwrap();
        heap.allocate(JvmRole::Object).unwrap();
        assert!(heap.allocate(JvmRole::Object).is_err());
        assert_eq!(heap.live_len(), 1);
    }

    fn direct_fixture(
        target_flags: u16,
        method_flags: u16,
    ) -> (
        ClassLoader,
        Arc<ClassDefinition>,
        JvmHeap,
        ManagedHandle,
        ManagedHandle,
    ) {
        let loader = ClassLoader::new(4096);
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let target = ClassDefinition::test(
            loader.id(),
            "target.Target",
            2,
            JavaClassMetadata::test_class(
                &cx,
                "target.Target",
                &[],
                target_flags,
                &[("run", "()V", method_flags)],
            ),
            BTreeMap::new(),
        );
        let owner = ClassDefinition::test(
            loader.id(),
            "caller.Owner",
            1,
            JavaClassMetadata::test_class(&cx, "caller.Owner", &[], 0x0001, &[]),
            BTreeMap::from([(
                7,
                SymbolicConstant::Member {
                    kind: ConstantResolutionKind::Method,
                    binary_name: "target.Target".into(),
                    name: "run".into(),
                    descriptor: "()V".into(),
                },
            )]),
        );
        loader.test_insert(target);
        loader.test_insert(owner.clone());
        let mut heap = JvmHeap::new(
            8,
            CollectionLimits {
                objects: 8,
                edges: 8,
                stack: 8,
                work: 32,
                clears: 8,
                finalizers: 0,
            },
        )
        .unwrap();
        let cache = heap.allocate(JvmRole::Cache).unwrap();
        let owner_handle = heap.allocate(JvmRole::ClassMirror).unwrap();
        (loader, owner, heap, cache, owner_handle)
    }

    #[test]
    fn static_direct_handle_defers_initialization_until_invocation() {
        let (loader, owner, mut heap, cache, owner_handle) = direct_fixture(0x0001, 0x0009);
        let handle = resolve_direct_handle(
            &ResolutionCache::new(),
            &mut heap,
            cache,
            owner_handle,
            &loader,
            &owner,
            7,
            6,
            DirectReceiver::None,
        )
        .unwrap();
        assert_eq!(handle.kind(), DirectInvocationKind::Static);
        assert!(handle.initializes_on_invocation());
        assert_eq!(handle.declaring_class().id().loader(), loader.id());
    }

    #[test]
    fn interruption_stage_preserves_resume_and_performs_one_pipeline_effect_per_attempt() {
        struct Pipeline {
            calls: usize,
            resumes: Vec<Option<u8>>,
        }

        impl LambdaMethodPipeline for Pipeline {
            type Resume = u8;
            type Exception = sim_lib_control::Raised;

            fn invoke(
                &mut self,
                call: LambdaMethodCall<'_, Self::Resume>,
            ) -> Result<LambdaInvocationOutcome<Self::Resume, Self::Exception>, InvocationError>
            {
                self.calls += 1;
                self.resumes.push(call.resume);
                Ok(match call.resume {
                    None => LambdaInvocationOutcome::Interrupted {
                        resume: 41,
                        work: 7,
                    },
                    Some(41) => LambdaInvocationOutcome::Returned {
                        value: None,
                        work: 11,
                    },
                    Some(other) => panic!("linker changed resume evidence to {other}"),
                })
            }
        }

        let (loader, owner, mut heap, cache, owner_handle) = direct_fixture(0x0001, 0x0009);
        let implementation = resolve_direct_handle(
            &ResolutionCache::new(),
            &mut heap,
            cache,
            owner_handle,
            &loader,
            &owner,
            7,
            6,
            DirectReceiver::None,
        )
        .unwrap();
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let site = SiteKey {
            class: owner.id().clone(),
            method: MethodIdentity {
                name: "make".into(),
                descriptor: "()Lexample/Function;".into(),
            },
            constant_pool_index: 7,
            bootstrap: BootstrapMethod {
                method_handle: 3,
                arguments: Box::new([]),
            },
        };
        let functional = FunctionalInterface {
            interface: "example.Function".into(),
            method_name: "apply".into(),
            method_descriptor: "()V".into(),
            lineage: vec!["example.Function".into()],
        };
        let bootstrap = LambdaBootstrapPlan {
            sam_method_type: "()V".into(),
            implementation_reference_kind: 6,
            instantiated_method_type: "()V".into(),
            marker_interfaces: vec![],
            bridges: vec![],
            serializable: false,
        };
        let class = GeneratedLambdaClassSpace::new()
            .define(
                &cx,
                &mut heap,
                &loader,
                &owner,
                &site,
                "()Lexample/Function;",
                &functional,
                &bootstrap,
            )
            .unwrap();
        let plan = JvmFunctionPlan {
            neutral: neutral_plan(0, 0),
            body: JvmFunctionPolicyBody {
                adaptations: Box::new([]),
            },
        };
        let mut pipeline = Pipeline {
            calls: 0,
            resumes: vec![],
        };

        let interrupted = invoke_lambda_member(
            &mut pipeline,
            &class,
            &plan,
            &implementation,
            "apply",
            "()V",
            &[],
            vec![],
            None,
        )
        .unwrap();
        assert!(matches!(
            interrupted,
            LambdaInvocationOutcome::Interrupted {
                resume: 41,
                work: 7
            }
        ));
        let resumed = invoke_lambda_member(
            &mut pipeline,
            &class,
            &plan,
            &implementation,
            "apply",
            "()V",
            &[],
            vec![],
            Some(41),
        )
        .unwrap();
        assert!(matches!(
            resumed,
            LambdaInvocationOutcome::Returned {
                value: None,
                work: 11
            }
        ));
        assert_eq!(pipeline.calls, 2);
        assert_eq!(pipeline.resumes, [None, Some(41)]);
    }

    #[test]
    fn inaccessible_direct_target_fails_during_normative_resolution() {
        let (loader, owner, mut heap, cache, owner_handle) = direct_fixture(0, 0x0009);
        assert_eq!(
            resolve_direct_handle(
                &ResolutionCache::new(),
                &mut heap,
                cache,
                owner_handle,
                &loader,
                &owner,
                7,
                6,
                DirectReceiver::None,
            )
            .unwrap_err(),
            DirectHandleError::Resolution(ConstantResolutionError::IllegalAccess {
                binary_name: "target.Target".into(),
            })
        );
    }

    #[test]
    fn receiver_rules_and_unsupported_kinds_fail_closed() {
        let (loader, owner, mut heap, cache, owner_handle) = direct_fixture(0x0001, 0x0001);
        assert!(matches!(
            resolve_direct_handle(
                &ResolutionCache::new(),
                &mut heap,
                cache,
                owner_handle,
                &loader,
                &owner,
                7,
                5,
                DirectReceiver::None,
            ),
            Err(DirectHandleError::MissingReceiverRule)
        ));
        assert!(matches!(
            resolve_direct_handle(
                &ResolutionCache::new(),
                &mut heap,
                cache,
                owner_handle,
                &loader,
                &owner,
                7,
                1,
                DirectReceiver::None,
            ),
            Err(DirectHandleError::UnsupportedReferenceKind(1))
        ));
    }
}
