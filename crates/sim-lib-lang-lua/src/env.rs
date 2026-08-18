use sim_kernel::{Result, Symbol, Value};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use sim_lib_binding::{BindingCell, LexicalEnv};
use sim_lib_function::CapturedBinding;
use sim_lib_gc_tracing::ManagedHeap;
use sim_lib_mutation::{ManagedHandle, ManagedNode};

const LUA_BINDING_LIMIT: usize = 4096;

/// Lexical local environment used by the Lua core eval policy.
#[derive(Clone)]
pub struct LuaEnv {
    lexical: LexicalEnv,
    managed: Arc<Mutex<ManagedHeap<ManagedNode<()>>>>,
    handles: Arc<ManagedBindingFrame>,
}

struct ManagedBindingFrame {
    parent: Option<Arc<ManagedBindingFrame>>,
    slots: Mutex<BTreeMap<Symbol, ManagedHandle>>,
}

impl Default for LuaEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl LuaEnv {
    /// Build an empty Lua local environment.
    pub fn new() -> Self {
        Self {
            lexical: LexicalEnv::new(),
            managed: Arc::new(Mutex::new(
                ManagedHeap::retaining(LUA_BINDING_LIMIT)
                    .expect("the Lua binding limit is nonzero"),
            )),
            handles: Arc::new(ManagedBindingFrame {
                parent: None,
                slots: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// Open a nested scope whose lookups fall through to this one.
    pub fn child(&self) -> Self {
        Self {
            lexical: self.lexical.child(),
            managed: Arc::clone(&self.managed),
            handles: Arc::new(ManagedBindingFrame {
                parent: Some(Arc::clone(&self.handles)),
                slots: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// Bind a Lua local value in the current frame.
    pub fn define(&mut self, name: Symbol, value: Value) -> Result<()> {
        self.lexical.define(name.clone(), value)?;
        let handle = self
            .managed
            .lock()
            .map_err(|_| sim_kernel::Error::PoisonedLock("lua managed bindings"))?
            .allocate(ManagedNode::new(()))
            .map_err(|error| {
                sim_kernel::Error::Eval(format!("cannot allocate Lua binding: {error}"))
            })?;
        self.handles
            .slots
            .lock()
            .map_err(|_| sim_kernel::Error::PoisonedLock("lua managed binding frame"))?
            .insert(name, handle);
        Ok(())
    }

    /// Return whether a Lua local is bound.
    pub fn contains(&self, name: &Symbol) -> bool {
        self.lexical.lookup(name).is_ok()
    }

    /// Assign an existing Lua local.
    pub fn assign(&mut self, name: &Symbol, value: Value) -> Result<Value> {
        self.capture(name)?.set(value.clone())?;
        Ok(value)
    }

    /// Look up a Lua local value.
    pub fn get(&self, name: &Symbol) -> Result<Value> {
        self.lexical.lookup(name)
    }

    /// Capture an existing Lua local as a shared upvalue cell.
    pub fn capture(&self, name: &Symbol) -> Result<BindingCell> {
        self.lexical.capture_cell(name)
    }

    /// Capture an existing local as one shared cell with its managed identity.
    pub fn capture_managed(&self, name: &Symbol) -> Result<CapturedBinding> {
        let cell = self.capture(name)?;
        let mut frame = Some(Arc::clone(&self.handles));
        while let Some(current) = frame {
            if let Some(handle) = current
                .slots
                .lock()
                .map_err(|_| sim_kernel::Error::PoisonedLock("lua managed binding frame"))?
                .get(name)
                .copied()
            {
                return Ok(CapturedBinding::new(cell, handle));
            }
            frame = current.parent.as_ref().map(Arc::clone);
        }
        Err(sim_kernel::Error::Eval(format!(
            "lua binding {name} has no managed identity"
        )))
    }
}
