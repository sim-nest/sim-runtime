use std::{collections::BTreeMap, sync::Arc};

use sim_kernel::{Ref, Symbol};
use sim_lib_control::{Generator, GeneratorStep};
use sim_lib_pattern::{AlgebraicDataType, PatternField, VariantDeclaration};
use sim_shape::AnyShape;

/// Builds the profile's `Option` algebraic data type over the pattern organ.
///
/// Declares the nullary `None` and unary `Some` variants as an
/// [`AlgebraicDataType`]; matching and construction are provided by
/// [`sim_lib_pattern`], not this profile.
pub fn typed_lazy_option_type() -> AlgebraicDataType {
    AlgebraicDataType::new(
        Symbol::qualified("typed-lazy/adt", "Option"),
        vec![
            VariantDeclaration::nullary(Symbol::qualified("typed-lazy/option", "None")),
            VariantDeclaration::new(
                Symbol::qualified("typed-lazy/option", "Some"),
                vec![PatternField::new(Symbol::new("value"), Arc::new(AnyShape))],
            ),
        ],
    )
    .expect("typed-lazy option variants are unique")
}

/// Explicit dictionary mapping a typeclass instance's method names to implementation symbols.
///
/// This profile models typeclasses as explicit dictionaries rather than inferred
/// resolution; the corresponding fidelity badge is marked limited.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeclassDictionary {
    class: Symbol,
    instance: Symbol,
    methods: BTreeMap<Symbol, Symbol>,
}

impl TypeclassDictionary {
    /// Creates an empty dictionary for the given class and instance type.
    pub fn new(class: Symbol, instance: Symbol) -> Self {
        Self {
            class,
            instance,
            methods: BTreeMap::new(),
        }
    }

    /// Records the implementation symbol for a method name, returning `self`.
    pub fn add_method(mut self, name: Symbol, implementation: Symbol) -> Self {
        self.methods.insert(name, implementation);
        self
    }

    /// Looks up the implementation symbol bound to a method name.
    pub fn method(&self, name: &Symbol) -> Option<&Symbol> {
        self.methods.get(name)
    }

    /// Returns the typeclass symbol this dictionary instantiates.
    pub fn class(&self) -> &Symbol {
        &self.class
    }

    /// Returns the instance (type) symbol this dictionary is bound to.
    pub fn instance(&self) -> &Symbol {
        &self.instance
    }
}

/// A thunk that defers a single [`Ref`] and caches it once forced.
///
/// Models the profile's limited laziness over the control organ's [`Generator`]:
/// the first [`force`](LazyRef::force) yields and memoizes the value, later
/// forces return the cached value.
///
/// # Examples
///
/// ```
/// use sim_kernel::{Ref, Symbol};
/// use sim_lib_lang_typed_lazy::LazyRef;
///
/// let value = Ref::Symbol(Symbol::qualified("lazy", "value"));
/// let mut lazy = LazyRef::new(value.clone());
/// assert!(!lazy.is_forced());
/// assert_eq!(lazy.force(), value);
/// assert!(lazy.is_forced());
/// assert_eq!(lazy.force(), value); // cached on the second force
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LazyRef {
    generator: Generator,
    cached: Option<Ref>,
}

impl LazyRef {
    /// Creates an unforced thunk over a single deferred [`Ref`].
    pub fn new(value: Ref) -> Self {
        Self {
            generator: Generator::new(vec![value]),
            cached: None,
        }
    }

    /// Forces the thunk, returning and memoizing its value.
    pub fn force(&mut self) -> Ref {
        if let Some(value) = &self.cached {
            return value.clone();
        }
        let value = match self.generator.next_step() {
            GeneratorStep::Yielded(value) => value,
            GeneratorStep::Exhausted => Ref::Symbol(Symbol::qualified("typed-lazy", "exhausted")),
        };
        self.cached = Some(value.clone());
        value
    }

    /// Reports whether the thunk has already been forced.
    pub fn is_forced(&self) -> bool {
        self.cached.is_some()
    }
}
