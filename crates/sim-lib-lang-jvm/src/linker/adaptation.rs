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

type JavaLambdaInvoker =
    dyn Fn(&mut Cx, Vec<JvmValue>) -> JavaLambdaCallOutcome + Send + Sync;

/// A linked Java lambda projected through the kernel `FUNCTION_2` callable boundary.
pub struct JavaLambdaCallable {
    argument_shapes: Vec<ShapeRef>,
    result_shape: Option<ShapeRef>,
    invoke: Arc<JavaLambdaInvoker>,
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
    if let (Some(from), Some(to)) = (primitive(from), primitive(to))
        && primitive_widens(from, to)
    {
        return Ok(JvmAdaptation::PrimitiveWiden { from, to });
    }
    if let (Some(primitive), true) = (primitive(from), is_reference(to))
        && (wrapper_primitive(to).is_some_and(|wrapped| wrapped == primitive)
            || to == "Ljava/lang/Object;"
        )
    {
        return Ok(JvmAdaptation::Box {
            primitive,
            reference: to.into(),
        });
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
