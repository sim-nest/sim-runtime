//! Language-neutral runtime policy kit for guest profiles.

use std::{fmt, sync::Arc};

use sim_kernel::{Cx, Result, Value};

/// How a guest language decides whether a runtime value is truthy.
pub trait TruthPolicy: Send + Sync {
    /// Return whether `value` counts as truthy for this guest profile.
    fn is_truthy(&self, cx: &mut Cx, value: &Value) -> Result<bool>;
}

/// How a guest language coerces values at number and string boundaries.
pub trait CoercionPolicy: Send + Sync {
    /// Convert `value` to a number value when this profile accepts such a coercion.
    fn to_number(&self, cx: &mut Cx, value: &Value) -> Result<Option<Value>>;

    /// Convert `value` to a string value when this profile accepts such a coercion.
    fn to_string(&self, cx: &mut Cx, value: &Value) -> Result<Option<Value>>;
}

/// Multivalue arity rule applied at a guest language boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arity {
    /// Keep exactly this many values, padding with the profile nil value.
    Exact(usize),
    /// Keep one value, padding with the profile nil value when none were returned.
    AtLeastOne,
    /// Keep every returned value.
    All,
}

/// Apply `rule` to returned values, using `nil` when padding is needed.
pub fn adjust_values(mut values: Vec<Value>, rule: Arity, nil: Value) -> Vec<Value> {
    match rule {
        Arity::All => values,
        Arity::AtLeastOne => {
            if values.is_empty() {
                vec![nil]
            } else {
                values.truncate(1);
                values
            }
        }
        Arity::Exact(count) => {
            values.resize(count, nil);
            values
        }
    }
}

/// Runtime policy bundle a language profile configures once and reuses.
#[derive(Clone)]
pub struct GuestRuntimeKit {
    /// Truthiness policy for this profile.
    pub truth: Arc<dyn TruthPolicy>,
    /// Boundary coercion policy for this profile.
    pub coerce: Arc<dyn CoercionPolicy>,
    /// Profile-specific nil value used for arity padding.
    pub nil: Value,
}

impl GuestRuntimeKit {
    /// Build a kit from truthiness, coercion, and nil policies.
    pub fn new(truth: Arc<dyn TruthPolicy>, coerce: Arc<dyn CoercionPolicy>, nil: Value) -> Self {
        Self { truth, coerce, nil }
    }

    /// Return whether `value` counts as truthy for this profile.
    pub fn is_truthy(&self, cx: &mut Cx, value: &Value) -> Result<bool> {
        self.truth.is_truthy(cx, value)
    }

    /// Convert `value` to a number value when this profile accepts such a coercion.
    pub fn to_number(&self, cx: &mut Cx, value: &Value) -> Result<Option<Value>> {
        self.coerce.to_number(cx, value)
    }

    /// Convert `value` to a string value when this profile accepts such a coercion.
    pub fn to_string(&self, cx: &mut Cx, value: &Value) -> Result<Option<Value>> {
        self.coerce.to_string(cx, value)
    }

    /// Apply an arity rule using this profile's nil value for padding.
    pub fn adjust_values(&self, values: Vec<Value>, rule: Arity) -> Vec<Value> {
        adjust_values(values, rule, self.nil.clone())
    }
}

impl fmt::Debug for GuestRuntimeKit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GuestRuntimeKit")
            .field("nil", &self.nil)
            .finish_non_exhaustive()
    }
}
