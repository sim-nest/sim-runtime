use std::{collections::BTreeMap, sync::Arc};

use sim_kernel::{Args, Cx, Error, Ref, Result, Shape, Symbol, Value};
use sim_lib_binding::{LexicalEnv, eval_let, lexical_function_value};
use sim_lib_control::{
    Condition, ConditionHandler, ConditionStack, ContinuationValue, ControlResultValue, Restart,
    RestartStack, invoke_restart, signal_condition,
};
use sim_lib_dispatch::{
    DispatchMethod, GenericFunction, MethodBody, MethodRole, MethodSpecificity,
};
use sim_lib_mutation::{Cell, cell_value};
use sim_lib_namespace::{Namespace, NamespaceKind};

use crate::cl_lite_package_symbol;

/// Native body of a CL-lite function or macro.
///
/// Receives the call context, the captured lexical environment, and the
/// argument values, and returns a runtime [`Value`].
pub type ClFunctionBody =
    Arc<dyn Fn(&mut Cx, &LexicalEnv, Vec<Value>) -> Result<Value> + Send + Sync + 'static>;

/// CL-lite evaluation state: lexical environment, macro table, special-variable
/// cells, and the current package.
///
/// Each surface form delegates to a shared runtime organ rather than
/// reimplementing behavior.
pub struct ClLiteRuntime {
    env: LexicalEnv,
    macros: BTreeMap<Symbol, Value>,
    variables: BTreeMap<Symbol, Value>,
    package: Namespace,
}

impl ClLiteRuntime {
    /// Creates an empty runtime with the CL-lite package installed.
    ///
    /// # Examples
    ///
    /// ```
    /// use sim_lib_lang_cl::ClLiteRuntime;
    ///
    /// let runtime = ClLiteRuntime::new().unwrap();
    /// assert_eq!(runtime.package().symbol().to_string(), "common-lisp/lite");
    /// ```
    pub fn new() -> Result<Self> {
        Ok(Self {
            env: LexicalEnv::new(),
            macros: BTreeMap::new(),
            variables: BTreeMap::new(),
            package: cl_lite_package()?,
        })
    }

    /// Returns the runtime's lexical environment.
    pub fn environment(&self) -> &LexicalEnv {
        &self.env
    }

    /// Returns the runtime's current package namespace.
    pub fn package(&self) -> &Namespace {
        &self.package
    }

    /// Defines a function in the lexical environment via the binding organ.
    pub fn defun(&mut self, cx: &mut Cx, name: Symbol, body: ClFunctionBody) -> Result<Value> {
        let value = lexical_function_value(cx, name.clone(), self.env.clone(), body)?;
        self.env.define(name, value.clone())?;
        Ok(value)
    }

    /// Looks up a defined function by name in the lexical environment.
    pub fn function(&self, name: &Symbol) -> Result<Value> {
        self.env.lookup(name)
    }

    /// Defines a macro function in the macro table via the binding organ.
    pub fn defmacro(&mut self, cx: &mut Cx, name: Symbol, body: ClFunctionBody) -> Result<Value> {
        let value = lexical_function_value(cx, name.clone(), self.env.clone(), body)?;
        self.macros.insert(name, value.clone());
        Ok(value)
    }

    /// Looks up a macro function by name, if one is defined.
    pub fn macro_function(&self, name: &Symbol) -> Option<Value> {
        self.macros.get(name).cloned()
    }

    /// Evaluates `body` in a lexical frame extended with `bindings`.
    ///
    /// Delegates to the binding organ's `let` evaluation.
    pub fn let_form(
        &self,
        cx: &mut Cx,
        bindings: Vec<(Symbol, Value)>,
        body: impl FnOnce(&mut Cx, &LexicalEnv) -> Result<Value>,
    ) -> Result<Value> {
        eval_let(cx, &self.env, bindings, body)
    }

    /// Defines a special variable backed by a mutation cell.
    pub fn define_variable(&mut self, cx: &mut Cx, name: Symbol, initial: Value) -> Result<Value> {
        let cell = cell_value(cx, initial)?;
        self.variables.insert(name, cell.clone());
        Ok(cell)
    }

    /// Updates a defined variable's cell; requires the mutation capability.
    pub fn setq(&mut self, cx: &mut Cx, name: &Symbol, value: Value) -> Result<Value> {
        let cell_value = self
            .variables
            .get(name)
            .ok_or_else(|| Error::Eval(format!("CL-lite variable {name} is not defined")))?;
        let cell = cell_value
            .object()
            .downcast_ref::<Cell>()
            .ok_or_else(|| Error::Eval(format!("CL-lite variable {name} is not mutable")))?;
        cell.set(cx, value.clone())?;
        Ok(value)
    }

    /// Reads the current value of a defined variable's cell.
    pub fn variable_value(&self, name: &Symbol) -> Result<Value> {
        let cell_value = self
            .variables
            .get(name)
            .ok_or_else(|| Error::Eval(format!("CL-lite variable {name} is not defined")))?;
        let cell = cell_value
            .object()
            .downcast_ref::<Cell>()
            .ok_or_else(|| Error::Eval(format!("CL-lite variable {name} is not mutable")))?;
        cell.get()
    }

    /// Updates a generalized place by writing into its mutation cell.
    pub fn setf_cell(&mut self, cx: &mut Cx, cell: &Cell, value: Value) -> Result<Value> {
        cell.set(cx, value.clone())?;
        Ok(value)
    }
}

/// Calls a callable runtime value with the given arguments.
///
/// Errors if `value` is not callable.
pub fn call_cl_value(cx: &mut Cx, value: &Value, args: Vec<Value>) -> Result<Value> {
    let callable = value
        .object()
        .as_callable()
        .ok_or_else(|| Error::Eval("CL-lite value is not callable".to_owned()))?;
    callable.call(cx, Args::new(args))
}

/// Condition-and-restart scope for CL-lite `handler-case` / `restart-case`.
///
/// Holds the active handler and restart stacks; all signaling delegates to the
/// control organ.
pub struct ClLiteControlScope {
    conditions: ConditionStack,
    restarts: RestartStack,
}

impl Default for ClLiteControlScope {
    fn default() -> Self {
        Self::new()
    }
}

impl ClLiteControlScope {
    /// Creates an empty control scope with no handlers or restarts.
    pub fn new() -> Self {
        Self {
            conditions: ConditionStack::new(),
            restarts: RestartStack::new(),
        }
    }

    /// Pushes a condition handler onto the handler stack.
    pub fn push_handler(&mut self, handler: ConditionHandler) {
        self.conditions.push(handler);
    }

    /// Pops the most recently pushed condition handler.
    pub fn pop_handler(&mut self) -> Option<ConditionHandler> {
        self.conditions.pop()
    }

    /// Signals a condition to the nearest matching handler.
    pub fn handler_case(
        &self,
        cx: &mut Cx,
        kind: Symbol,
        payload: Ref,
    ) -> Result<ContinuationValue> {
        signal_condition(cx, &self.conditions, Condition::new(kind, payload))
    }

    /// Pushes a named restart with its resumption continuation.
    pub fn push_restart(&mut self, name: Symbol, continuation: ContinuationValue) {
        self.restarts.push(Restart::new(name, continuation));
    }

    /// Pops the most recently pushed restart.
    pub fn pop_restart(&mut self) -> Option<Restart> {
        self.restarts.pop()
    }

    /// Invokes a named restart with a resumption value.
    pub fn restart_case(
        &self,
        cx: &mut Cx,
        name: &Symbol,
        value: Ref,
    ) -> Result<ControlResultValue> {
        invoke_restart(cx, &self.restarts, name, value)
    }
}

/// CL-lite generic function (`defgeneric` / `defmethod`) over the dispatch organ.
pub struct ClGenericFunction {
    generic: GenericFunction,
}

impl ClGenericFunction {
    /// Creates a named generic function with no methods.
    pub fn new(name: Symbol) -> Self {
        Self {
            generic: GenericFunction::new(name),
        }
    }

    /// Returns the generic function's name.
    pub fn name(&self) -> &Symbol {
        self.generic.name()
    }

    /// Adds a primary method keyed by its parameter shapes.
    pub fn add_primary_method(
        &mut self,
        id: Symbol,
        parameter_shapes: Vec<Arc<dyn Shape>>,
        body: MethodBody,
    ) -> Result<()> {
        self.generic.add_method(DispatchMethod::new(
            id,
            MethodRole::Primary,
            parameter_shapes,
            body,
        ))
    }

    /// Selects the most specific primary method for the given arguments.
    pub fn select_primary(&self, cx: &mut Cx, args: &[Value]) -> Result<MethodSpecificity> {
        self.generic.select_primary(cx, args)
    }

    /// Returns the applicable methods in dispatch order for the arguments.
    pub fn dispatch_order(&self, cx: &mut Cx, args: &[Value]) -> Result<Vec<Symbol>> {
        self.generic.dispatch_order(cx, args)
    }

    /// Dispatches and calls the generic function under the CL-lite profile.
    pub fn call(&self, cx: &mut Cx, args: &[Value]) -> Result<Value> {
        self.generic
            .call_for_profile(cx, &crate::cl_lite_profile_symbol(), args)
    }
}

/// Builds the CL-lite package namespace exporting the surface form symbols.
pub fn cl_lite_package() -> Result<Namespace> {
    let mut package = Namespace::package(cl_lite_package_symbol());
    for name in [
        "defun",
        "defmacro",
        "let",
        "setq",
        "handler-case",
        "restart-case",
        "defgeneric",
        "defmethod",
        "defpackage",
        "in-package",
        "setf",
    ] {
        let local = Symbol::new(name);
        package.define(local.clone(), Symbol::qualified("cl", name))?;
        package.export(local)?;
    }
    debug_assert_eq!(package.kind(), NamespaceKind::Package);
    Ok(package)
}
