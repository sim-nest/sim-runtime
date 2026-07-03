use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard},
};

use sim_kernel::{Error, Result, Symbol, Value};

type DynamicFrame = BTreeMap<Symbol, Value>;

/// A stack of dynamic-extent binding frames shared across clones.
///
/// Holds the fluid (dynamically scoped) bindings of the binding organ. Frames
/// are pushed for the duration of a body and popped when it returns or unwinds,
/// giving dynamic rather than lexical extent.
#[derive(Clone, Debug, Default)]
pub struct DynamicEnv {
    frames: Arc<Mutex<Vec<DynamicFrame>>>,
}

impl DynamicEnv {
    /// Creates an empty dynamic environment with no active frames.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the innermost binding for `name`, or `None` if it is unbound.
    pub fn lookup(&self, name: &Symbol) -> Result<Option<Value>> {
        Ok(self
            .frames()?
            .iter()
            .rev()
            .find_map(|frame| frame.get(name).cloned()))
    }

    /// Runs `body` with `bindings` installed in a fresh frame.
    ///
    /// The frame is popped when `body` returns, including on early return or
    /// non-local unwind, so the bindings have dynamic extent only.
    pub fn with_bindings<T>(
        &self,
        bindings: Vec<(Symbol, Value)>,
        body: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        self.frames()?.push(bindings.into_iter().collect());
        let _guard = DynamicFrameGuard {
            frames: self.frames.clone(),
        };
        body()
    }

    fn frames(&self) -> Result<MutexGuard<'_, Vec<DynamicFrame>>> {
        self.frames
            .lock()
            .map_err(|_| Error::Eval("dynamic binding frame lock is poisoned".to_owned()))
    }
}

struct DynamicFrameGuard {
    frames: Arc<Mutex<Vec<DynamicFrame>>>,
}

impl Drop for DynamicFrameGuard {
    fn drop(&mut self) {
        if let Ok(mut frames) = self.frames.lock() {
            frames.pop();
        }
    }
}

/// A dynamic parameter: a named fluid binding with a fallback default.
///
/// Backed by a [`DynamicEnv`], a parameter reads the innermost dynamic binding
/// for its name and falls back to its default when none is active. This is the
/// binding organ's surface for `parameterize`-style rebinding.
#[derive(Clone, Debug)]
pub struct Parameter {
    name: Symbol,
    default: Value,
    dynamic: DynamicEnv,
}

impl Parameter {
    /// Creates a parameter over a fresh [`DynamicEnv`] with the given default.
    pub fn new(name: Symbol, default: Value) -> Self {
        Self::with_dynamic_env(name, default, DynamicEnv::new())
    }

    /// Creates a parameter bound to an existing [`DynamicEnv`].
    ///
    /// Use this to share one environment across parameters that must observe
    /// each other's frames.
    pub fn with_dynamic_env(name: Symbol, default: Value, dynamic: DynamicEnv) -> Self {
        Self {
            name,
            default,
            dynamic,
        }
    }

    /// Returns the parameter's name.
    pub fn name(&self) -> &Symbol {
        &self.name
    }

    /// Returns the current value: the innermost binding, or the default.
    pub fn get(&self) -> Result<Value> {
        Ok(self
            .dynamic
            .lookup(&self.name)?
            .unwrap_or_else(|| self.default.clone()))
    }

    /// Runs `body` with the parameter rebound to `value` for that dynamic extent.
    ///
    /// The previous value is restored when `body` returns or unwinds.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Symbol};
    /// use sim_lib_binding::Parameter;
    ///
    /// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    /// let default = cx.factory().symbol(Symbol::new("default")).unwrap();
    /// let temporary = cx.factory().symbol(Symbol::new("temporary")).unwrap();
    /// let parameter = Parameter::new(Symbol::new("current"), default.clone());
    ///
    /// parameter
    ///     .with_value(temporary.clone(), || {
    ///         assert_eq!(parameter.get()?, temporary);
    ///         Ok(())
    ///     })
    ///     .unwrap();
    /// assert_eq!(parameter.get().unwrap(), default);
    /// ```
    pub fn with_value<T>(&self, value: Value, body: impl FnOnce() -> Result<T>) -> Result<T> {
        self.dynamic
            .with_bindings(vec![(self.name.clone(), value)], body)
    }
}
