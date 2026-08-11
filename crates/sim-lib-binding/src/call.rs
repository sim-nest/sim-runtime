//! Language-neutral call argument partitioning and binding.

use std::collections::{BTreeMap, BTreeSet};

use sim_kernel::{Error, Result, Symbol, Value};

/// One argument at a neutral call boundary.
#[derive(Clone, Debug)]
pub enum CallArgument {
    /// An argument assigned by declaration order.
    Positional(Value),
    /// An argument assigned by parameter name.
    Named(Symbol, Value),
}

/// Whether unmatched arguments in a partition are rejected or collected.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum Remainder {
    /// Reject unmatched arguments.
    #[default]
    Prohibited,
    /// Collect unmatched arguments under this binding name.
    Variadic(Symbol),
}

/// A declared parameter, optionally carrying its default value.
#[derive(Clone, Debug)]
pub struct CallParameter {
    name: Symbol,
    default: Option<Value>,
}

impl CallParameter {
    /// Declares a required parameter.
    pub fn required(name: Symbol) -> Self {
        Self {
            name,
            default: None,
        }
    }

    /// Declares a parameter whose value is used when the caller omits it.
    pub fn defaulted(name: Symbol, value: Value) -> Self {
        Self {
            name,
            default: Some(value),
        }
    }

    /// Returns the parameter name.
    pub fn name(&self) -> &Symbol {
        &self.name
    }
}

/// A language-neutral call signature.
///
/// Positional parameters are filled in declaration order. Named parameters are
/// addressable only by name. Each unmatched partition is independently either
/// prohibited or collected into a variadic binding.
#[derive(Clone, Debug, Default)]
pub struct CallSignature {
    positional: Vec<CallParameter>,
    named: Vec<CallParameter>,
    positional_remainder: Remainder,
    named_remainder: Remainder,
}

impl CallSignature {
    /// Creates an empty signature which prohibits all arguments.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the ordered positional partition.
    pub fn with_positional(mut self, parameters: Vec<CallParameter>) -> Self {
        self.positional = parameters;
        self
    }

    /// Replaces the named partition.
    pub fn with_named(mut self, parameters: Vec<CallParameter>) -> Self {
        self.named = parameters;
        self
    }

    /// Selects how unmatched positional arguments are handled.
    pub fn with_positional_remainder(mut self, remainder: Remainder) -> Self {
        self.positional_remainder = remainder;
        self
    }

    /// Selects how unmatched named arguments are handled.
    pub fn with_named_remainder(mut self, remainder: Remainder) -> Self {
        self.named_remainder = remainder;
        self
    }

    /// Validates the declaration and binds a call without invoking its body.
    pub fn bind(&self, arguments: impl IntoIterator<Item = CallArgument>) -> Result<BoundCall> {
        self.validate()?;
        let mut supplied_named = BTreeMap::new();
        let mut positional_values = Vec::new();
        let mut saw_named = false;

        for argument in arguments {
            match argument {
                CallArgument::Positional(value) => {
                    if saw_named {
                        return Err(call_error(
                            "ordering",
                            "positional argument follows a named argument",
                        ));
                    }
                    positional_values.push(value);
                }
                CallArgument::Named(name, value) => {
                    saw_named = true;
                    if supplied_named.insert(name.clone(), value).is_some() {
                        return Err(call_error("duplicate", format!("named argument {name}")));
                    }
                }
            }
        }

        let mut bindings = BTreeMap::new();
        for (index, parameter) in self.positional.iter().enumerate() {
            let positional = positional_values.get(index).cloned();
            let named = supplied_named.remove(&parameter.name);
            let value = match (positional, named, parameter.default.clone()) {
                (Some(_), Some(_), _) => {
                    return Err(call_error(
                        "duplicate",
                        format!(
                            "parameter {} supplied positionally and by name",
                            parameter.name
                        ),
                    ));
                }
                (Some(value), None, _) | (None, Some(value), _) => value,
                (None, None, Some(value)) => value,
                (None, None, None) => {
                    return Err(call_error(
                        "missing",
                        format!("required parameter {}", parameter.name),
                    ));
                }
            };
            bindings.insert(parameter.name.clone(), value);
        }

        for parameter in &self.named {
            let value = match supplied_named.remove(&parameter.name) {
                Some(value) => value,
                None => parameter.default.clone().ok_or_else(|| {
                    call_error(
                        "missing",
                        format!("required named parameter {}", parameter.name),
                    )
                })?,
            };
            bindings.insert(parameter.name.clone(), value);
        }

        let extra_positional = positional_values
            .into_iter()
            .skip(self.positional.len())
            .collect::<Vec<_>>();
        if !extra_positional.is_empty() && self.positional_remainder == Remainder::Prohibited {
            return Err(call_error(
                "unexpected",
                format!("{} positional argument(s)", extra_positional.len()),
            ));
        }
        if !supplied_named.is_empty() && self.named_remainder == Remainder::Prohibited {
            let names = supplied_named
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(call_error(
                "unexpected",
                format!("named argument(s): {names}"),
            ));
        }

        Ok(BoundCall {
            bindings,
            positional_remainder: extra_positional,
            named_remainder: supplied_named,
        })
    }

    fn validate(&self) -> Result<()> {
        let mut names = BTreeSet::new();
        for parameter in self.positional.iter().chain(&self.named) {
            if !names.insert(parameter.name.clone()) {
                return Err(call_error(
                    "duplicate",
                    format!("parameter declaration {}", parameter.name),
                ));
            }
        }
        for remainder in [&self.positional_remainder, &self.named_remainder] {
            if let Remainder::Variadic(name) = remainder
                && !names.insert(name.clone())
            {
                return Err(call_error(
                    "duplicate",
                    format!("variadic declaration {name}"),
                ));
            }
        }
        Ok(())
    }
}

/// The complete result of binding a call signature.
#[derive(Clone, Debug)]
pub struct BoundCall {
    bindings: BTreeMap<Symbol, Value>,
    positional_remainder: Vec<Value>,
    named_remainder: BTreeMap<Symbol, Value>,
}

impl BoundCall {
    /// Returns the value assigned to a declared parameter.
    pub fn get(&self, name: &Symbol) -> Option<&Value> {
        self.bindings.get(name)
    }

    /// Returns unmatched positional values collected by a variadic partition.
    pub fn positional_remainder(&self) -> &[Value] {
        &self.positional_remainder
    }

    /// Returns unmatched named values in stable name order.
    pub fn named_remainder(&self) -> &BTreeMap<Symbol, Value> {
        &self.named_remainder
    }
}

fn call_error(category: &str, detail: impl std::fmt::Display) -> Error {
    Error::Eval(format!("call binding {category}: {detail}"))
}
