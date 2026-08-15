//! A checked function-organ specimen for a language that does not exist yet.

use std::sync::Arc;

use sim_kernel::{
    Callable, ClassRef, Cx, Expr, ObjectCompat, Result, Shape, ShapeId, Symbol, Value,
    force_list_to_vec,
    shape::{MatchScore, ShapeDoc, ShapeMatch},
    testing::bare_cx,
};
use sim_lib_binding::BindingCell;
use sim_lib_dispatch::{DispatchMethod, GenericFunction, MethodRole};
use sim_lib_function::{
    ArgumentInput, ArgumentOrigin, BoundCall, CallInput, CallMode, CaptureDescriptor,
    CapturedBinding, FunctionBodyPolicy, FunctionInstance, FunctionPlan, ParameterDescriptor,
    ParameterKind, bind, dispatch_method_body,
};
use sim_lib_gc_tracing::{CollectionLimits, ManagedHeap};
use sim_lib_mutation::{EdgeId, EdgeVisitor, ManagedId, ManagedNode, ManagedObject};

#[derive(Clone)]
struct ToyBody;

impl FunctionBodyPolicy for ToyBody {
    fn invoke(
        &self,
        cx: &mut Cx,
        _plan: &FunctionPlan,
        captures: &[CapturedBinding],
        call: BoundCall,
    ) -> Result<Value> {
        let captured = captures[0]
            .cell()
            .get()
            .expect("the specimen initializes its lexical capture");
        let positional = call
            .positional()
            .next()
            .and_then(|argument| match argument.input() {
                ArgumentInput::Positional(value) => Some(value.clone()),
                _ => None,
            })
            .expect("the toy language requires one positional argument");
        let named = call
            .named()
            .next()
            .and_then(|argument| match argument.input() {
                ArgumentInput::Named { name, value } if name == &Symbol::new("suffix") => {
                    Some(value.clone())
                }
                _ => None,
            });

        let mut parts = vec![captured, positional];
        if let Some(named) = named {
            parts.push(named);
        }
        cx.factory().list(parts)
    }
}

#[derive(Clone)]
enum CycleObject {
    Function(FunctionInstance<ToyBody>),
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

struct AcceptAnything;

impl Shape for AcceptAnything {
    fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(0)))
    }

    fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(0)))
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("toy-value"))
    }
}

fn limits() -> CollectionLimits {
    CollectionLimits {
        objects: 4,
        edges: 4,
        stack: 4,
        work: 32,
        clears: 4,
        finalizers: 4,
    }
}

fn plan() -> FunctionPlan {
    FunctionPlan::new(
        Symbol::new("toy:join"),
        vec![
            ParameterDescriptor::new(
                Symbol::new("value"),
                ParameterKind::Required,
                CallMode::POSITIONAL,
                Some(ShapeId(41)),
            ),
            ParameterDescriptor::new(
                Symbol::new("suffix"),
                ParameterKind::Optional,
                CallMode::NAMED,
                Some(ShapeId(42)),
            ),
        ],
        vec![CaptureDescriptor::new(
            Symbol::new("prefix"),
            Some(ShapeId(40)),
        )],
        Some(ShapeId(43)),
    )
    .unwrap()
}

fn symbol(cx: &mut Cx, name: &str) -> Value {
    cx.factory().symbol(Symbol::new(name)).unwrap()
}

#[test]
fn neutral_language_composes_the_complete_function_organ() {
    let mut cx = bare_cx();
    let prefix = symbol(&mut cx, "captured");
    let positional = symbol(&mut cx, "positional");
    let suffix = symbol(&mut cx, "named");
    let class: ClassRef = symbol(&mut cx, "toy-function");
    let args_shape = symbol(&mut cx, "toy-args-shape");
    let result_shape = symbol(&mut cx, "toy-result-shape");
    let cell = BindingCell::initialized(Symbol::new("prefix"), prefix.clone());
    let mut heap = ManagedHeap::tracing(4, limits()).unwrap();
    let environment = heap
        .allocate(CycleObject::Environment(ManagedNode::new(())))
        .unwrap();
    let function = FunctionInstance::new(
        plan(),
        ToyBody,
        vec![CapturedBinding::new(cell, environment)],
        class.clone(),
        Some(args_shape.clone()),
        Some(result_shape.clone()),
    )
    .unwrap();

    let named_call = bind(
        CallInput::new()
            .with(
                ArgumentInput::Positional(positional.clone()),
                ArgumentOrigin::Guest(Symbol::new("toy:arg/0")),
            )
            .with(
                ArgumentInput::Named {
                    name: Symbol::new("suffix"),
                    value: suffix.clone(),
                },
                ArgumentOrigin::Guest(Symbol::new("toy:arg/suffix")),
            ),
    );
    let named_result = function
        .body()
        .invoke(&mut cx, function.plan(), function.captures(), named_call)
        .unwrap();
    let named_values = force_list_to_vec(
        &mut cx,
        named_result.object().as_list().unwrap(),
        "toy named result",
    )
    .unwrap();
    assert_eq!(
        named_values,
        vec![prefix.clone(), positional.clone(), suffix]
    );
    assert_eq!(function.class(&mut cx).unwrap(), class);
    assert_eq!(
        function.browse_args_shape(&mut cx).unwrap(),
        Some(args_shape)
    );
    assert_eq!(
        function.browse_result_shape(&mut cx).unwrap(),
        Some(result_shape)
    );
    assert_eq!(
        function.plan().browse().parameters(),
        &[
            (Symbol::new("value"), Some(ShapeId(41))),
            (Symbol::new("suffix"), Some(ShapeId(42))),
        ]
    );

    let mut generic = GenericFunction::new(Symbol::new("toy:generic"));
    generic
        .add_method(DispatchMethod::new(
            Symbol::new("toy:join-method"),
            MethodRole::Primary,
            vec![Arc::new(AcceptAnything)],
            dispatch_method_body(function.clone()),
        ))
        .unwrap();
    let dispatch_result = generic
        .call(&mut cx, std::slice::from_ref(&positional))
        .unwrap();
    let dispatch_values = force_list_to_vec(
        &mut cx,
        dispatch_result.object().as_list().unwrap(),
        "toy dispatch result",
    )
    .unwrap();
    assert_eq!(dispatch_values, vec![prefix, positional]);

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

#[test]
fn specimen_has_no_established_guest_runtime_dependency() {
    let source = include_str!("neutral_language_specimen.rs");
    for parts in [
        ["sim_lib_lang_", "java", "script"].as_slice(),
        ["sim_lib_lang_", "py", "thon"].as_slice(),
        ["sim_lib_lang_", "lu", "a"].as_slice(),
        ["sim_lib_lang_", "j", "vm"].as_slice(),
    ] {
        assert!(!source.contains(&parts.concat()));
    }
}
