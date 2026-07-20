use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use sim_kernel::{Cx, Expr, NumberLiteral, Result, Symbol, Value};

/// A table key derived from a runtime value rather than a kernel [`Symbol`].
///
/// Guest languages use this when table/hash/map keys are ordinary values. The
/// key remains policy-controlled so each language decides which values are
/// admissible and how numeric values collapse or stay distinct.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RuntimeKey {
    /// Boolean key.
    Bool(bool),
    /// Integer key.
    Integer(i64),
    /// Floating-point key represented by its raw IEEE-754 bits.
    FloatBits(u64),
    /// Text key.
    Str(String),
    /// Symbol key.
    Symbol(Symbol),
    /// Object identity key, local to this runtime process.
    ObjectIdentity(u64),
}

impl RuntimeKey {
    /// Derives a general-purpose key from `value`.
    ///
    /// Nil and NaN are rejected with `None`; booleans, numbers, strings, and
    /// symbols become structural keys; every other value is keyed by object
    /// identity. Guest languages can call this from their own
    /// [`RuntimeKeyPolicy`] or provide a stricter mapping.
    pub fn from_value(cx: &mut Cx, value: &Value) -> Result<Option<Self>> {
        match value.object().as_expr(cx)? {
            Expr::Nil => Ok(None),
            Expr::Bool(value) => Ok(Some(Self::Bool(value))),
            Expr::Number(number) => Ok(number_key(&number)),
            Expr::String(value) => Ok(Some(Self::Str(value))),
            Expr::Symbol(symbol) => Ok(Some(Self::Symbol(symbol))),
            _ => Ok(Some(Self::ObjectIdentity(object_identity(value)))),
        }
    }

    /// Returns this key as a contiguous integer index when it is one.
    pub fn as_integer_index(&self) -> Option<i64> {
        match self {
            Self::Integer(index) => Some(*index),
            _ => None,
        }
    }

    /// Projects this key into an expression for inspection.
    pub fn as_expr(&self) -> Expr {
        match self {
            Self::Bool(value) => Expr::Bool(*value),
            Self::Integer(value) => Expr::Number(NumberLiteral {
                domain: Symbol::qualified("runtime-key", "integer"),
                canonical: value.to_string(),
            }),
            Self::FloatBits(bits) => Expr::Number(NumberLiteral {
                domain: Symbol::qualified("runtime-key", "float-bits"),
                canonical: bits.to_string(),
            }),
            Self::Str(value) => Expr::String(value.clone()),
            Self::Symbol(symbol) => Expr::Symbol(symbol.clone()),
            Self::ObjectIdentity(identity) => Expr::Extension {
                tag: Symbol::qualified("mutation", "object-identity-key"),
                payload: Box::new(Expr::String(identity.to_string())),
            },
        }
    }
}

/// Maps a language value to a runtime table key.
///
/// Returning `None` means the language forbids that value as a key. For
/// example, a policy can reject nil, NaN, or any object kind it cannot identify.
pub trait RuntimeKeyPolicy: Send + Sync {
    /// Returns the runtime key for `value`, or `None` when the key is forbidden.
    fn key_for(&self, cx: &mut Cx, value: &Value) -> Result<Option<RuntimeKey>>;
}

/// A small reusable key policy for languages that accept primitive keys and
/// object identity.
#[derive(Clone, Copy, Debug, Default)]
pub struct PrimitiveRuntimeKeyPolicy;

impl RuntimeKeyPolicy for PrimitiveRuntimeKeyPolicy {
    fn key_for(&self, cx: &mut Cx, value: &Value) -> Result<Option<RuntimeKey>> {
        RuntimeKey::from_value(cx, value)
    }
}

fn number_key(number: &NumberLiteral) -> Option<RuntimeKey> {
    if let Ok(value) = number.canonical.parse::<i64>() {
        return Some(RuntimeKey::Integer(value));
    }
    let value = number.canonical.parse::<f64>().ok()?;
    (!value.is_nan()).then_some(RuntimeKey::FloatBits(value.to_bits()))
}

fn object_identity(value: &Value) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}
