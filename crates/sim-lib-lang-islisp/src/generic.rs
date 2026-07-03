use std::{collections::BTreeMap, sync::Arc};

use sim_kernel::{Cx, Expr, Object, ObjectCompat, Result, Shape, Symbol, Value};
use sim_lib_dispatch::{
    DispatchMethod, GenericFunction, MethodBody, MethodRole, MethodSpecificity,
};

#[sim_citizen_derive::non_citizen(
    reason = "dynamic ISLISP instance shell; canonical data is the class symbol and slot table",
    kind = "marker"
)]
/// Runtime shell for an ISLISP instance: a class symbol plus a slot table.
///
/// A kernel [`Object`] that renders to the shared [`Expr`] graph; the canonical
/// data is the class symbol and slots, not this Rust struct.
#[derive(Clone, Debug)]
pub struct IslispObject {
    class: Symbol,
    slots: BTreeMap<Symbol, Value>,
}

impl IslispObject {
    /// Builds an instance from its class symbol and slot table.
    pub fn new(class: Symbol, slots: BTreeMap<Symbol, Value>) -> Self {
        Self { class, slots }
    }

    /// Returns the class symbol this instance was created against.
    pub fn class(&self) -> &Symbol {
        &self.class
    }

    /// Returns the instance slot table keyed by slot symbol.
    pub fn slots(&self) -> &BTreeMap<Symbol, Value> {
        &self.slots
    }
}

impl Object for IslispObject {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<islisp-object {}>", self.class))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for IslispObject {
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        let slots = self
            .slots
            .iter()
            .map(|(slot, value)| Ok((Expr::Symbol(slot.clone()), value.object().as_expr(cx)?)))
            .collect::<Result<Vec<_>>>()?;
        Ok(Expr::Map(vec![
            (
                Expr::Symbol(Symbol::new("class")),
                Expr::Symbol(self.class.clone()),
            ),
            (Expr::Symbol(Symbol::new("slots")), Expr::Map(slots)),
        ]))
    }

    fn truth(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(true)
    }
}

/// Wraps an [`IslispObject`] as an opaque kernel [`Value`].
///
/// Allocates the instance through the context factory so it participates in the
/// runtime like any other object value.
pub fn islisp_object_value(
    cx: &mut Cx,
    class: Symbol,
    slots: BTreeMap<Symbol, Value>,
) -> Result<Value> {
    cx.factory()
        .opaque(Arc::new(IslispObject::new(class, slots)))
}

/// ISLISP generic function backed by the shared dispatch organ.
///
/// Thin profile wrapper over [`GenericFunction`]: it adds the ISLISP surface
/// shape, while method selection and shape-based dispatch remain dispatch-organ
/// behavior rather than kernel contract.
pub struct IslispGeneric {
    generic: GenericFunction,
}

impl IslispGeneric {
    /// Creates an empty generic function with the given name.
    pub fn new(name: Symbol) -> Self {
        Self {
            generic: GenericFunction::new(name),
        }
    }

    /// Returns the generic function's name symbol.
    pub fn name(&self) -> &Symbol {
        self.generic.name()
    }

    /// Attaches a primary method keyed by its identifier and parameter shapes.
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

    /// Returns the applicable primary methods ordered from most to least specific.
    pub fn dispatch_order(&self, cx: &mut Cx, args: &[Value]) -> Result<Vec<Symbol>> {
        self.generic.dispatch_order(cx, args)
    }

    /// Dispatches the generic on the given arguments and returns the result.
    pub fn call(&self, cx: &mut Cx, args: &[Value]) -> Result<Value> {
        self.generic.call(cx, args)
    }
}
