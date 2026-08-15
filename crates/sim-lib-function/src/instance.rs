use std::{any::Any, error::Error, fmt};

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Object, ObjectCompat, Result as KernelResult, ShapeRef, Value,
};
use sim_lib_binding::BindingCell;
use sim_lib_mutation::{
    EdgeId, EdgeVisitor, ManagedHandle, ManagedId, ManagedNode, ManagedObject,
    StrongEdgeMutationError,
};

use crate::{BoundCall, CallInput, FunctionPlan, bind};

/// Guest-owned execution policy for one concrete function body type.
///
/// The policy is statically selected by [`FunctionInstance`]. It receives the
/// neutral call record and shared capture cells, leaving defaults, keyword
/// rules, receiver behavior, evaluation, and diagnostics to the guest.
pub trait FunctionBodyPolicy: Send + Sync + 'static {
    /// Executes this body using the immutable declaration and live captures.
    fn invoke(
        &self,
        cx: &mut Cx,
        plan: &FunctionPlan,
        captures: &[CapturedBinding],
        call: BoundCall,
    ) -> KernelResult<Value>;
}

/// One shared binding cell paired with its identity in the managed graph.
#[derive(Clone, Debug)]
pub struct CapturedBinding {
    cell: BindingCell,
    managed: ManagedHandle,
}

impl CapturedBinding {
    /// Associates an existing binding cell with its managed allocation.
    pub const fn new(cell: BindingCell, managed: ManagedHandle) -> Self {
        Self { cell, managed }
    }

    /// Borrows the shared lexical cell.
    pub const fn cell(&self) -> &BindingCell {
        &self.cell
    }

    /// Returns the managed identity traced for this capture.
    pub const fn managed(&self) -> ManagedHandle {
        self.managed
    }
}

/// Failure to construct a managed function instance.
#[derive(Debug)]
pub enum InstanceError {
    /// Capture cells must exactly follow the plan's declared slots.
    CaptureMismatch {
        /// Number of capture descriptors in the plan.
        expected: usize,
        /// Number of supplied managed binding cells.
        actual: usize,
    },
    /// A supplied cell did not match the corresponding frozen capture slot.
    CaptureNameMismatch {
        /// Zero-based position in the frozen capture sequence.
        index: usize,
        /// Name declared by the immutable function plan.
        expected: String,
        /// Name carried by the supplied shared binding cell.
        actual: String,
    },
    /// The shared managed node refused a capture edge.
    ManagedEdge(StrongEdgeMutationError),
}

impl fmt::Display for InstanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CaptureMismatch { expected, actual } => write!(
                formatter,
                "function plan declares {expected} captures but {actual} were supplied"
            ),
            Self::CaptureNameMismatch {
                index,
                expected,
                actual,
            } => write!(
                formatter,
                "function capture {index} is named {actual}, expected {expected}"
            ),
            Self::ManagedEdge(error) => write!(formatter, "cannot trace function capture: {error}"),
        }
    }
}

impl Error for InstanceError {}

/// The typed payload retained by a managed function node.
#[derive(Clone)]
struct FunctionRole<B> {
    plan: FunctionPlan,
    body: B,
    captures: Vec<CapturedBinding>,
    class: ClassRef,
    args_shape: Option<ShapeRef>,
    result_shape: Option<ShapeRef>,
}

/// A language-neutral, managed function object with a concrete guest body.
///
/// Each capture is represented twice for distinct purposes: its existing
/// [`BindingCell`] supplies shared lexical mutation, while its `ManagedHandle`
/// becomes a strong edge in the common managed graph. No private environment
/// graph or body registry is involved.
#[derive(Clone)]
pub struct FunctionInstance<B: FunctionBodyPolicy> {
    node: ManagedNode<FunctionRole<B>>,
}

impl<B: FunctionBodyPolicy> FunctionInstance<B> {
    /// Builds an instance from a plan, typed body, managed captures, and runtime metadata.
    pub fn new(
        plan: FunctionPlan,
        body: B,
        captures: Vec<CapturedBinding>,
        class: ClassRef,
        args_shape: Option<ShapeRef>,
        result_shape: Option<ShapeRef>,
    ) -> Result<Self, InstanceError> {
        if plan.captures().len() != captures.len() {
            return Err(InstanceError::CaptureMismatch {
                expected: plan.captures().len(),
                actual: captures.len(),
            });
        }
        for (index, (descriptor, capture)) in plan.captures().iter().zip(&captures).enumerate() {
            if descriptor.name() != capture.cell().name() {
                return Err(InstanceError::CaptureNameMismatch {
                    index,
                    expected: descriptor.name().to_string(),
                    actual: capture.cell().name().to_string(),
                });
            }
        }
        let targets = captures
            .iter()
            .map(|capture| capture.managed().id())
            .collect::<Vec<_>>();
        let mut node = ManagedNode::new(FunctionRole {
            plan,
            body,
            captures,
            class,
            args_shape,
            result_shape,
        });
        for target in targets {
            node.insert_strong(target)
                .map_err(InstanceError::ManagedEdge)?;
        }
        Ok(Self { node })
    }

    /// Borrows the immutable declaration plan.
    pub const fn plan(&self) -> &FunctionPlan {
        &self.node.role().plan
    }

    /// Borrows the concrete guest body policy without erasure or downcasting.
    pub const fn body(&self) -> &B {
        &self.node.role().body
    }

    /// Borrows the capture cells in plan declaration order.
    pub fn captures(&self) -> &[CapturedBinding] {
        &self.node.role().captures
    }

    /// Borrows the caller-supplied runtime class.
    pub const fn supplied_class(&self) -> &ClassRef {
        &self.node.role().class
    }

    /// Borrows the caller-supplied argument Shape, when present.
    pub const fn args_shape(&self) -> Option<&ShapeRef> {
        self.node.role().args_shape.as_ref()
    }

    /// Borrows the caller-supplied result Shape, when present.
    pub const fn result_shape(&self) -> Option<&ShapeRef> {
        self.node.role().result_shape.as_ref()
    }

    /// Invokes the guest policy through the neutral evaluated-value boundary.
    ///
    /// Kernel calls and optional dispatch-method adaptation both use this path,
    /// so neither surface can change the policy-visible [`BoundCall`].
    pub fn invoke_bound(&self, cx: &mut Cx, call: BoundCall) -> KernelResult<Value> {
        self.body().invoke(cx, self.plan(), self.captures(), call)
    }

    pub(crate) fn invoke_values(&self, cx: &mut Cx, values: Vec<Value>) -> KernelResult<Value> {
        self.invoke_bound(cx, bind(CallInput::from(Args::new(values))))
    }
}

impl<B: FunctionBodyPolicy> ManagedObject for FunctionInstance<B> {
    fn trace_edges(&self, visitor: &mut dyn EdgeVisitor) {
        self.node.trace_edges(visitor);
    }

    fn clear_weak_edge(&mut self, edge: EdgeId, expected: ManagedId) -> bool {
        self.node.clear_weak_edge(edge, expected)
    }

    fn clear_ephemeron_edge(
        &mut self,
        edge: EdgeId,
        expected_key: ManagedId,
        expected_value: ManagedId,
    ) -> bool {
        self.node
            .clear_ephemeron_edge(edge, expected_key, expected_value)
    }
}

impl<B: FunctionBodyPolicy> Object for FunctionInstance<B> {
    fn display(&self, _cx: &mut Cx) -> KernelResult<String> {
        Ok(format!("#<function {}>", self.plan().display_identity()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<B: FunctionBodyPolicy> ObjectCompat for FunctionInstance<B> {
    fn class(&self, _cx: &mut Cx) -> KernelResult<ClassRef> {
        Ok(self.supplied_class().clone())
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl<B: FunctionBodyPolicy> Callable for FunctionInstance<B> {
    fn call(&self, cx: &mut Cx, args: Args) -> KernelResult<Value> {
        self.invoke_values(cx, args.into_vec())
    }

    fn browse_args_shape(&self, _cx: &mut Cx) -> KernelResult<Option<ShapeRef>> {
        Ok(self.args_shape().cloned())
    }

    fn browse_result_shape(&self, _cx: &mut Cx) -> KernelResult<Option<ShapeRef>> {
        Ok(self.result_shape().cloned())
    }
}

#[cfg(test)]
mod tests {
    use sim_kernel::{ShapeId, Symbol, testing::bare_cx};
    use sim_lib_gc_tracing::{CollectionLimits, ManagedHeap};
    use sim_lib_mutation::{EdgeSnapshot, ManagedNode};

    use super::*;
    use crate::CaptureDescriptor;

    #[derive(Clone)]
    struct EchoBody;

    impl FunctionBodyPolicy for EchoBody {
        fn invoke(
            &self,
            _cx: &mut Cx,
            _plan: &FunctionPlan,
            _captures: &[CapturedBinding],
            call: BoundCall,
        ) -> KernelResult<Value> {
            match call.arguments()[0].input() {
                crate::ArgumentInput::Positional(value) => Ok(value.clone()),
                _ => unreachable!("kernel arguments are positional"),
            }
        }
    }

    fn plan(captures: usize) -> FunctionPlan {
        FunctionPlan::new(
            Symbol::new("guest:echo"),
            Vec::new(),
            (0..captures)
                .map(|index| CaptureDescriptor::new(Symbol::new(format!("slot-{index}")), None))
                .collect(),
            Some(ShapeId(9)),
        )
        .unwrap()
    }

    fn metadata(cx: &mut Cx) -> (ClassRef, ShapeRef, ShapeRef) {
        (
            cx.factory().symbol(Symbol::new("guest-class")).unwrap(),
            cx.factory().symbol(Symbol::new("args-shape")).unwrap(),
            cx.factory().symbol(Symbol::new("result-shape")).unwrap(),
        )
    }

    fn collection_limits() -> CollectionLimits {
        CollectionLimits {
            objects: 4,
            edges: 4,
            stack: 4,
            work: 32,
            clears: 4,
            finalizers: 4,
        }
    }

    #[test]
    fn invocation_and_runtime_metadata_are_delegated_without_body_erasure() {
        let mut cx = bare_cx();
        let (class, args_shape, result_shape) = metadata(&mut cx);
        let instance = FunctionInstance::new(
            plan(0),
            EchoBody,
            Vec::new(),
            class.clone(),
            Some(args_shape.clone()),
            Some(result_shape.clone()),
        )
        .unwrap();
        let argument = cx.factory().symbol(Symbol::new("answer")).unwrap();

        assert!(std::ptr::eq(instance.body(), &instance.node.role().body));
        assert_eq!(instance.class(&mut cx).unwrap(), class);
        assert_eq!(
            instance.browse_args_shape(&mut cx).unwrap(),
            Some(args_shape)
        );
        assert_eq!(
            instance.browse_result_shape(&mut cx).unwrap(),
            Some(result_shape)
        );
        assert_eq!(
            instance
                .call(&mut cx, Args::new(vec![argument.clone()]))
                .unwrap(),
            argument
        );
    }

    #[test]
    fn same_plan_instances_receive_distinct_managed_identities() {
        let mut cx = bare_cx();
        let (class, _, _) = metadata(&mut cx);
        let mut heap = ManagedHeap::tracing(4, collection_limits()).unwrap();
        let first = heap
            .allocate(
                FunctionInstance::new(plan(0), EchoBody, vec![], class.clone(), None, None)
                    .unwrap(),
            )
            .unwrap();
        let second = heap
            .allocate(FunctionInstance::new(plan(0), EchoBody, vec![], class, None, None).unwrap())
            .unwrap();

        assert_ne!(first.id(), second.id());
    }

    #[derive(Clone)]
    enum CycleObject {
        Function(FunctionInstance<EchoBody>),
        Environment(ManagedNode<()>),
    }

    impl ManagedObject for CycleObject {
        fn trace_edges(&self, visitor: &mut dyn EdgeVisitor) {
            match self {
                Self::Function(function) => function.trace_edges(visitor),
                Self::Environment(environment) => environment.trace_edges(visitor),
            }
        }

        fn clear_weak_edge(&mut self, edge: EdgeId, expected: ManagedId) -> bool {
            match self {
                Self::Function(function) => function.clear_weak_edge(edge, expected),
                Self::Environment(environment) => environment.clear_weak_edge(edge, expected),
            }
        }

        fn clear_ephemeron_edge(
            &mut self,
            edge: EdgeId,
            expected_key: ManagedId,
            expected_value: ManagedId,
        ) -> bool {
            match self {
                Self::Function(function) => {
                    function.clear_ephemeron_edge(edge, expected_key, expected_value)
                }
                Self::Environment(environment) => {
                    environment.clear_ephemeron_edge(edge, expected_key, expected_value)
                }
            }
        }
    }

    #[test]
    fn closure_environment_cycle_is_collected_through_capture_edge() {
        let mut cx = bare_cx();
        let (class, _, _) = metadata(&mut cx);
        let mut heap = ManagedHeap::tracing(4, collection_limits()).unwrap();
        let environment = heap
            .allocate(CycleObject::Environment(ManagedNode::new(())))
            .unwrap();
        let cell = BindingCell::uninitialized(Symbol::new("slot-0"));
        let function = FunctionInstance::new(
            plan(1),
            EchoBody,
            vec![CapturedBinding::new(cell, environment)],
            class,
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            function.node.edge_snapshot(),
            vec![EdgeSnapshot::Strong {
                edge: EdgeId(0),
                target: environment.id(),
            }]
        );
        let function = heap.allocate(CycleObject::Function(function)).unwrap();
        match heap.get_mut(environment).unwrap() {
            CycleObject::Environment(node) => {
                node.insert_strong(function.id()).unwrap();
            }
            CycleObject::Function(_) => unreachable!(),
        }

        let receipt = heap.collect().unwrap().unwrap();
        assert_eq!(receipt.swept, vec![environment.id(), function.id()]);
        assert_eq!(heap.live_len(), 0);
    }
}
