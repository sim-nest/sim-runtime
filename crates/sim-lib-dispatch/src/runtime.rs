//! Runtime integration for the dispatch organ: a [`GenericFunction`] as a
//! first-class callable value.
//!
//! runtime dispatch organ. The dispatch machinery ([`GenericFunction::call`],
//! most-specific selection) is complete; this wraps a generic as a kernel
//! [`Callable`] so it is an ordinary runtime value the evaluator can invoke.
//! Calling it dispatches on the evaluated arguments and runs the single
//! most-specific applicable primary method. Generics are constructed dynamically
//! (there is no fixed symbol to register), so the organ's runtime surface is this
//! value wrapper rather than a loadable set of named functions.

use std::any::Any;
use std::sync::Arc;

use sim_kernel::{Args, Callable, ClassRef, Cx, Object, ObjectCompat, Result, Symbol, Value};

use crate::generic::GenericFunction;

impl Object for GenericFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<generic {}>", self.name()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ObjectCompat for GenericFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for GenericFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let values = args.into_vec();
        // Delegate to the inherent dispatch entry point (`&[Value]`).
        GenericFunction::call(self, cx, values.as_slice())
    }
}

/// Wraps `generic` as a runtime callable value that dispatches most-specific.
pub fn generic_function_value(cx: &mut Cx, generic: GenericFunction) -> Result<Value> {
    cx.factory().opaque(Arc::new(generic))
}
