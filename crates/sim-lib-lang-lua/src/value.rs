use sim_kernel::Value;

/// Result of evaluating a Lua core form.
#[derive(Clone, Debug)]
pub enum LuaResult {
    /// Ordinary expression values.
    Values(Vec<Value>),
    /// Values carried by a Lua `return` form.
    Return(Vec<Value>),
}

impl LuaResult {
    /// Build an ordinary single-value result.
    pub fn one(value: Value) -> Self {
        Self::Values(vec![value])
    }

    /// Build ordinary expression values.
    pub fn values(values: Vec<Value>) -> Self {
        Self::Values(values)
    }

    /// Build returned values.
    pub fn return_values(values: Vec<Value>) -> Self {
        Self::Return(values)
    }

    /// Borrow the contained values.
    pub fn values_ref(&self) -> &[Value] {
        match self {
            Self::Values(values) | Self::Return(values) => values,
        }
    }

    /// Return whether this result came from a Lua `return` form.
    pub fn is_return(&self) -> bool {
        matches!(self, Self::Return(_))
    }

    /// Consume the result and return its values.
    pub fn into_values(self) -> Vec<Value> {
        match self {
            Self::Values(values) | Self::Return(values) => values,
        }
    }
}
