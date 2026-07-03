use std::sync::Arc;

use sim_kernel::{Cx, Result, Shape, Symbol, Value};
use sim_lib_dispatch::{DispatchMethod, GenericFunction, MethodBody, MethodRole};

/// Julia multiple-dispatch function backed by the shared dispatch organ.
///
/// Thin profile wrapper over [`GenericFunction`]: it presents the Julia
/// surface, while argument specificity and method selection remain
/// dispatch-organ behavior rather than kernel contract.
pub struct JuliaFunction {
    generic: GenericFunction,
}

impl JuliaFunction {
    /// Creates an empty Julia function with the given name.
    pub fn new(name: Symbol) -> Self {
        Self {
            generic: GenericFunction::new(name),
        }
    }

    /// Adds a primary method keyed by its name and argument shapes.
    pub fn add_method(
        &mut self,
        method: Symbol,
        argument_shapes: Vec<Arc<dyn Shape>>,
        body: MethodBody,
    ) -> Result<()> {
        self.generic.add_method(DispatchMethod::new(
            method,
            MethodRole::Primary,
            argument_shapes,
            body,
        ))
    }

    /// Returns the applicable methods ordered from most to least specific.
    pub fn dispatch_order(&self, cx: &mut Cx, args: &[Value]) -> Result<Vec<Symbol>> {
        self.generic.dispatch_order(cx, args)
    }

    /// Dispatches under the Julia profile and returns the selected method's result.
    pub fn call(&self, cx: &mut Cx, args: &[Value]) -> Result<Value> {
        self.generic
            .call_for_profile(cx, &crate::julia_profile_symbol(), args)
    }
}
