use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
};

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Object, ObjectCompat, Result, Symbol, Value,
};

use crate::BindingCell;

type BindingSlot = Arc<Mutex<Option<Value>>>;

/// Computes a binding's initial value within a (possibly partial) scope.
///
/// Used by `let*` and `letrec` so each initializer can observe the bindings
/// already established in the same frame.
pub type BindingInitializer =
    Box<dyn Fn(&mut Cx, &LexicalEnv) -> Result<Value> + Send + Sync + 'static>;

type LexicalBody =
    Arc<dyn Fn(&mut Cx, &LexicalEnv, Vec<Value>) -> Result<Value> + Send + Sync + 'static>;

/// A lexical scope: a frame of name-to-value slots chained to its parent.
///
/// Cloning shares the same frame; [`child`](LexicalEnv::child) opens a nested
/// scope. Slots support deferred initialization so `letrec` can predefine names
/// before computing their values.
#[derive(Clone, Debug)]
pub struct LexicalEnv {
    frame: Arc<LexicalFrame>,
}

#[derive(Debug)]
struct LexicalFrame {
    parent: Option<LexicalEnv>,
    slots: Mutex<BTreeMap<Symbol, BindingSlot>>,
}

impl Default for LexicalEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl LexicalEnv {
    /// Creates a fresh root scope with no parent and no bindings.
    pub fn new() -> Self {
        Self {
            frame: Arc::new(LexicalFrame {
                parent: None,
                slots: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// Opens a nested scope whose lookups fall through to this one.
    pub fn child(&self) -> Self {
        Self {
            frame: Arc::new(LexicalFrame {
                parent: Some(self.clone()),
                slots: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// Binds `name` to `value` in this frame.
    ///
    /// Errors if `name` is already bound in the same frame (shadowing requires
    /// a [`child`](LexicalEnv::child) scope).
    pub fn define(&self, name: Symbol, value: Value) -> Result<()> {
        self.define_slot(name, Some(value))
    }

    /// Resolves `name` through this frame and its parents.
    ///
    /// Errors if the name is undefined or was predefined but never initialized.
    pub fn lookup(&self, name: &Symbol) -> Result<Value> {
        let Some(slot) = self.lookup_slot(name)? else {
            return Err(Error::Eval(format!(
                "lexical binding {name} is not defined"
            )));
        };
        slot.lock()
            .map_err(|_| Error::Eval(format!("lexical binding {name} lock is poisoned")))?
            .clone()
            .ok_or_else(|| Error::Eval(format!("lexical binding {name} is not initialized")))
    }

    /// Captures `name` as a shared cell for closure formation.
    ///
    /// Mutating the returned cell updates the lexical slot itself, so every
    /// closure that captures the same binding observes the same value.
    pub fn capture_cell(&self, name: &Symbol) -> Result<BindingCell> {
        let Some(slot) = self.lookup_slot(name)? else {
            return Err(Error::Eval(format!(
                "lexical binding {name} is not defined"
            )));
        };
        Ok(BindingCell::from_slot(name.clone(), slot))
    }

    fn predefine(&self, name: Symbol) -> Result<()> {
        self.define_slot(name, None)
    }

    fn set(&self, name: &Symbol, value: Value) -> Result<()> {
        let Some(slot) = self.lookup_slot(name)? else {
            return Err(Error::Eval(format!(
                "lexical binding {name} is not defined"
            )));
        };
        *slot
            .lock()
            .map_err(|_| Error::Eval(format!("lexical binding {name} lock is poisoned")))? =
            Some(value);
        Ok(())
    }

    fn define_slot(&self, name: Symbol, value: Option<Value>) -> Result<()> {
        let mut slots = self.slots()?;
        if slots.contains_key(&name) {
            return Err(Error::Eval(format!(
                "lexical binding {name} is already defined in this frame"
            )));
        }
        slots.insert(name, Arc::new(Mutex::new(value)));
        Ok(())
    }

    fn lookup_slot(&self, name: &Symbol) -> Result<Option<BindingSlot>> {
        if let Some(slot) = self.slots()?.get(name).cloned() {
            return Ok(Some(slot));
        }
        match &self.frame.parent {
            Some(parent) => parent.lookup_slot(name),
            None => Ok(None),
        }
    }

    fn slots(&self) -> Result<MutexGuard<'_, BTreeMap<Symbol, BindingSlot>>> {
        self.frame
            .slots
            .lock()
            .map_err(|_| Error::Eval("lexical binding frame lock is poisoned".to_owned()))
    }
}

/// Evaluates a `let` form: parallel bindings in a fresh child scope.
///
/// All values are supplied up front, so no binding can observe another; `body`
/// then runs in the child scope.
pub fn eval_let(
    cx: &mut Cx,
    outer: &LexicalEnv,
    bindings: Vec<(Symbol, Value)>,
    body: impl FnOnce(&mut Cx, &LexicalEnv) -> Result<Value>,
) -> Result<Value> {
    let env = outer.child();
    for (name, value) in bindings {
        env.define(name, value)?;
    }
    body(cx, &env)
}

/// Evaluates a `let*` form: sequential bindings in a fresh child scope.
///
/// Each [`BindingInitializer`] runs in order and sees the bindings established
/// before it.
pub fn eval_let_star(
    cx: &mut Cx,
    outer: &LexicalEnv,
    bindings: Vec<(Symbol, BindingInitializer)>,
    body: impl FnOnce(&mut Cx, &LexicalEnv) -> Result<Value>,
) -> Result<Value> {
    let env = outer.child();
    for (name, initializer) in bindings {
        let value = initializer(cx, &env)?;
        env.define(name, value)?;
    }
    body(cx, &env)
}

/// Evaluates a `letrec` form: mutually recursive bindings in a child scope.
///
/// All names are predefined before any initializer runs, so each
/// [`BindingInitializer`] may reference every binding in the frame (including
/// itself and later ones), enabling mutual recursion.
pub fn eval_letrec(
    cx: &mut Cx,
    outer: &LexicalEnv,
    bindings: Vec<(Symbol, BindingInitializer)>,
    body: impl FnOnce(&mut Cx, &LexicalEnv) -> Result<Value>,
) -> Result<Value> {
    let env = outer.child();
    let names = bindings
        .iter()
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    for name in &names {
        env.predefine(name.clone())?;
    }
    for ((_, initializer), name) in bindings.into_iter().zip(names.iter()) {
        let value = initializer(cx, &env)?;
        env.set(name, value)?;
    }
    body(cx, &env)
}

/// A closure that captures a [`LexicalEnv`] and is callable as a runtime object.
///
/// The kernel defines the `Object`/`Callable` contracts; this type realizes
/// them for a body that closes over its defining lexical scope. It is the
/// binding organ's representation of a lexically scoped function value.
#[derive(Clone)]
pub struct LexicalFunction {
    name: Symbol,
    env: LexicalEnv,
    body: LexicalBody,
}

impl LexicalFunction {
    /// Creates a function closing over `env`, identified by `name`.
    pub fn new(name: Symbol, env: LexicalEnv, body: LexicalBody) -> Self {
        Self { name, env, body }
    }

    /// Returns the function's name.
    pub fn name(&self) -> &Symbol {
        &self.name
    }
}

impl Object for LexicalFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<binding-function {}>", self.name))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LexicalFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LexicalFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        (self.body)(cx, &self.env, args.into_vec())
    }
}

/// Wraps a [`LexicalFunction`] as an opaque, callable runtime [`Value`].
///
/// The kernel factory defines opaque-object construction; this helper packages
/// a name, captured scope, and body into a callable value for the host eval.
pub fn lexical_function_value(
    cx: &mut Cx,
    name: Symbol,
    env: LexicalEnv,
    body: LexicalBody,
) -> Result<Value> {
    cx.factory()
        .opaque(Arc::new(LexicalFunction::new(name, env, body)))
}
