//! Python policy layered on the language-neutral function organ.

use std::{collections::BTreeMap, error::Error as StdError, fmt};

use sim_kernel::{Cx, Error, Result, Symbol, Value};
use sim_lib_function::{
    ArgumentInput, BoundCall, CapturedBinding, FunctionBodyPolicy, FunctionInstance, FunctionPlan,
};

use crate::Annotation;

/// Python-only signature rules that must not leak into neutral function plans.
#[derive(Clone, Debug, Default)]
pub struct PythonSignature {
    defaults: BTreeMap<Symbol, Value>,
}

impl PythonSignature {
    /// Creates a signature with declaration-time default objects.
    pub fn new(defaults: BTreeMap<Symbol, Value>) -> Self {
        Self { defaults }
    }

    /// Returns the exact declaration-time default object, preserving mutability identity.
    pub fn default(&self, name: &Symbol) -> Option<&Value> {
        self.defaults.get(name)
    }
}

/// Python execution flags retained by the language body policy.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PythonFunctionFlags {
    /// Whether invocation constructs a generator frame.
    pub generator: bool,
    /// Whether invocation constructs a coroutine frame.
    pub coroutine: bool,
    /// Whether descriptor access binds a receiver.
    pub descriptor: bool,
}

/// Python-owned body and diagnostic metadata.
#[derive(Clone, Debug)]
pub struct PythonBodyPolicy {
    /// Token body retained for direct evaluation.
    pub body: Vec<String>,
    /// Python signature defaults and binding policy.
    pub signature: PythonSignature,
    /// Retained Python annotations.
    pub annotations: BTreeMap<String, Annotation>,
    /// Generator, coroutine, and descriptor behavior flags.
    pub flags: PythonFunctionFlags,
    /// Stable source origin used for Python tracebacks.
    pub traceback_origin: String,
    /// Python exception class used for call-binding failures.
    pub call_error_class: String,
}

/// A Python call-binding failure with Python-owned diagnostic vocabulary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PythonCallError {
    /// Python exception class.
    pub class: String,
    /// Stable traceback source origin.
    pub traceback_origin: String,
    /// Human-readable Python binding error.
    pub message: String,
}

impl fmt::Display for PythonCallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} at {}: {}",
            self.class, self.traceback_origin, self.message
        )
    }
}

impl StdError for PythonCallError {}

impl PythonBodyPolicy {
    /// Applies Python positional/named/default rules to a neutral lossless call record.
    pub fn bind(
        &self,
        plan: &FunctionPlan,
        call: &BoundCall,
    ) -> std::result::Result<BTreeMap<Symbol, Value>, PythonCallError> {
        let mut result = BTreeMap::new();
        let mut positional = plan.parameters().iter().filter(|parameter| {
            parameter.call_mode().is_positional()
                && parameter.kind() != sim_lib_function::ParameterKind::Remainder
        });
        for argument in call.arguments() {
            let (parameter, value) = match argument.input() {
                ArgumentInput::Positional(value) => (positional.next(), value),
                ArgumentInput::Named { name, value } => (
                    plan.parameters().iter().find(|parameter| {
                        parameter.name() == name && parameter.call_mode().is_named()
                    }),
                    value,
                ),
                ArgumentInput::Receiver(value) => (positional.next(), value),
                ArgumentInput::Remainder(_) | ArgumentInput::Unconsumed(_) => {
                    return self.fail("unsupported expanded argument");
                }
            };
            let Some(parameter) = parameter else {
                return self.fail("unexpected argument");
            };
            if result
                .insert(parameter.name().clone(), value.clone())
                .is_some()
            {
                return self.fail(&format!(
                    "multiple values for argument {}",
                    parameter.name()
                ));
            }
        }
        for parameter in plan.parameters() {
            if !result.contains_key(parameter.name()) {
                if let Some(value) = self.signature.default(parameter.name()) {
                    result.insert(parameter.name().clone(), value.clone());
                } else if parameter.kind() == sim_lib_function::ParameterKind::Required {
                    return self.fail(&format!("missing required argument {}", parameter.name()));
                }
            }
        }
        Ok(result)
    }

    fn fail<T>(&self, message: &str) -> std::result::Result<T, PythonCallError> {
        Err(PythonCallError {
            class: self.call_error_class.clone(),
            traceback_origin: self.traceback_origin.clone(),
            message: message.to_owned(),
        })
    }
}

impl FunctionBodyPolicy for PythonBodyPolicy {
    fn invoke(
        &self,
        _cx: &mut Cx,
        plan: &FunctionPlan,
        _captures: &[CapturedBinding],
        call: BoundCall,
    ) -> Result<Value> {
        self.bind(plan, &call)
            .map_err(|error| Error::Eval(error.to_string()))?
            .into_values()
            .next()
            .ok_or_else(|| Error::Eval("python function body produced no value".into()))
    }
}

/// A Python function whose identity, plan, captures, callable surface, and reachability
/// are supplied exclusively by the shared function and managed organs.
pub type PythonFunction = FunctionInstance<PythonBodyPolicy>;

#[cfg(test)]
mod tests {
    use sim_kernel::{ClassRef, Symbol, testing::bare_cx};
    use sim_lib_binding::BindingCell;
    use sim_lib_function::{
        ArgumentInput, ArgumentOrigin, CallInput, CallMode, CaptureDescriptor, ParameterDescriptor,
        ParameterKind, bind,
    };
    use sim_lib_gc_tracing::{CollectionLimits, ManagedHeap};

    use super::*;
    use crate::{PythonManagedKind, PythonManagedObject};

    fn limits() -> CollectionLimits {
        CollectionLimits {
            objects: 16,
            edges: 32,
            stack: 16,
            work: 64,
            clears: 16,
            finalizers: 0,
        }
    }

    fn policy(defaults: BTreeMap<Symbol, Value>, generator: bool) -> PythonBodyPolicy {
        PythonBodyPolicy {
            body: vec!["return".into(), "value".into()],
            signature: PythonSignature::new(defaults),
            annotations: BTreeMap::new(),
            flags: PythonFunctionFlags {
                generator,
                ..PythonFunctionFlags::default()
            },
            traceback_origin: "fixture.py:4".into(),
            call_error_class: "TypeError".into(),
        }
    }

    #[test]
    fn keyword_only_and_mutable_default_are_python_policy() {
        let cx = bare_cx();
        let keyword_value = cx.factory().symbol(Symbol::new("keyword-value")).unwrap();
        let mutable_default = cx
            .factory()
            .symbol(Symbol::new("mutable-default-object"))
            .unwrap();
        let default_name = Symbol::new("items");
        let plan = FunctionPlan::new(
            Symbol::new("python:f"),
            vec![
                ParameterDescriptor::new(
                    Symbol::new("required_kw"),
                    ParameterKind::Required,
                    CallMode::NAMED,
                    None,
                ),
                ParameterDescriptor::new(
                    default_name.clone(),
                    ParameterKind::Optional,
                    CallMode::POSITIONAL_OR_NAMED,
                    None,
                ),
            ],
            vec![],
            None,
        )
        .unwrap();
        let body = policy(
            BTreeMap::from([(default_name.clone(), mutable_default.clone())]),
            false,
        );
        let call = bind(CallInput::new().with(
            ArgumentInput::Named {
                name: Symbol::new("required_kw"),
                value: keyword_value.clone(),
            },
            ArgumentOrigin::Guest(Symbol::new("fixture.py:8")),
        ));

        let first = body.bind(&plan, &call).unwrap();
        let second = body.bind(&plan, &call).unwrap();
        assert_eq!(first[&Symbol::new("required_kw")], keyword_value);
        assert_eq!(first[&default_name], mutable_default);
        assert_eq!(first[&default_name], second[&default_name]);
    }

    #[test]
    fn generator_function_uses_shared_plan_and_exact_managed_captures() {
        let cx = bare_cx();
        let captured_value = cx.factory().symbol(Symbol::new("captured-value")).unwrap();
        let class: ClassRef = cx.factory().symbol(Symbol::new("python-function")).unwrap();
        let capture_name = Symbol::new("closed_over");
        let plan = FunctionPlan::new(
            Symbol::new("python:generator"),
            vec![],
            vec![CaptureDescriptor::new(capture_name.clone(), None)],
            None,
        )
        .unwrap();
        let mut heap = ManagedHeap::tracing(4, limits()).unwrap();
        let environment = heap
            .allocate(PythonManagedObject::new(PythonManagedKind::Closure))
            .unwrap();
        let cell = BindingCell::initialized(capture_name.clone(), captured_value.clone());
        let function = PythonFunction::new(
            plan,
            policy(BTreeMap::new(), true),
            vec![CapturedBinding::new(cell, environment)],
            class,
            None,
            None,
        )
        .unwrap();

        assert!(function.body().flags.generator);
        assert_eq!(function.plan().captures()[0].name(), &capture_name);
        assert_eq!(function.captures()[0].cell().name(), &capture_name);
        assert_eq!(function.captures()[0].cell().get().unwrap(), captured_value);
    }
}
