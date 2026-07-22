use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

use sim_kernel::{
    Args, Callable, Cx, Error, Expr, Object, ObjectCompat, Result, Shape, Symbol, Value,
};

use crate::{AdtShape, VariantShape};

/// A named field of an ADT variant, carrying the kernel [`Shape`] that checks
/// and binds its value.
///
/// The kernel defines the [`Shape`] match/binding contract; a `PatternField`
/// pairs that contract with a field name so variant construction and matching
/// run the field's shape over the supplied value.
#[derive(Clone)]
pub struct PatternField {
    name: Symbol,
    shape: Arc<dyn Shape>,
}

impl PatternField {
    /// Builds a field with the given name and checking [`Shape`].
    pub fn new(name: Symbol, shape: Arc<dyn Shape>) -> Self {
        Self { name, shape }
    }

    /// Returns the field name.
    pub fn name(&self) -> &Symbol {
        &self.name
    }

    /// Returns the kernel [`Shape`] that checks and binds the field value.
    pub fn shape(&self) -> &Arc<dyn Shape> {
        &self.shape
    }
}

/// One named variant of an [`AlgebraicDataType`], with its ordered fields.
#[derive(Clone)]
pub struct VariantDeclaration {
    symbol: Symbol,
    fields: Vec<PatternField>,
}

impl VariantDeclaration {
    /// Builds a variant with the given tag symbol and ordered fields.
    pub fn new(symbol: Symbol, fields: Vec<PatternField>) -> Self {
        Self { symbol, fields }
    }

    /// Builds a field-less (nullary) variant such as an enum tag.
    pub fn nullary(symbol: Symbol) -> Self {
        Self {
            symbol,
            fields: Vec::new(),
        }
    }

    /// Returns the variant tag symbol.
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Returns the variant fields in declaration order.
    pub fn fields(&self) -> &[PatternField] {
        &self.fields
    }
}

/// A tagged-union type: a named set of variants, each with its own fields.
///
/// The kernel defines the [`Shape`] match/binding protocol; this type is the
/// pattern organ's concrete declaration of an algebraic data type. It produces
/// [`VariantConstructor`]s for building tagged values and a kernel [`Shape`]
/// ([`AdtShape`]) for matching them. See the [crate README] for the
/// protocol-versus-behavior boundary.
///
/// [crate README]: https://docs.rs/sim-runtime
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::Symbol;
/// use sim_lib_pattern::{AlgebraicDataType, VariantDeclaration};
///
/// let maybe = AlgebraicDataType::new(
///     Symbol::qualified("adt", "Maybe"),
///     vec![
///         VariantDeclaration::nullary(Symbol::qualified("maybe", "Nothing")),
///         VariantDeclaration::nullary(Symbol::qualified("maybe", "Just")),
///     ],
/// )
/// .unwrap();
/// assert_eq!(maybe.constructors().len(), 2);
/// assert!(maybe.constructor(&Symbol::qualified("maybe", "Just")).is_some());
/// ```
#[derive(Clone)]
pub struct AlgebraicDataType {
    symbol: Symbol,
    variants: BTreeMap<Symbol, VariantDeclaration>,
}

impl AlgebraicDataType {
    /// Builds an ADT from its name and variants.
    ///
    /// # Errors
    ///
    /// Returns an error if two variants share a tag symbol.
    pub fn new(symbol: Symbol, variants: Vec<VariantDeclaration>) -> Result<Self> {
        let mut by_symbol = BTreeMap::new();
        for variant in variants {
            if by_symbol
                .insert(variant.symbol().clone(), variant.clone())
                .is_some()
            {
                return Err(Error::Eval(format!(
                    "duplicate ADT variant {}",
                    variant.symbol()
                )));
            }
        }
        Ok(Self {
            symbol,
            variants: by_symbol,
        })
    }

    /// Returns the ADT name symbol.
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Iterates the declared variants in tag order.
    pub fn variants(&self) -> impl Iterator<Item = &VariantDeclaration> {
        self.variants.values()
    }

    /// Returns the constructor for `variant`, or `None` if it is not declared.
    pub fn constructor(&self, variant: &Symbol) -> Option<VariantConstructor> {
        self.variants
            .get(variant)
            .cloned()
            .map(|variant| VariantConstructor::new(self.symbol.clone(), variant))
    }

    /// Returns a constructor for every declared variant.
    pub fn constructors(&self) -> Vec<VariantConstructor> {
        self.variants
            .values()
            .cloned()
            .map(|variant| VariantConstructor::new(self.symbol.clone(), variant))
            .collect()
    }

    /// Returns the kernel [`Shape`] that matches any value of this ADT.
    pub fn shape(&self) -> Arc<dyn Shape> {
        Arc::new(AdtShape::new(
            self.symbol.clone(),
            self.constructors()
                .into_iter()
                .map(|constructor| constructor.variant_shape())
                .collect(),
        ))
    }
}

/// A callable builder for one ADT variant.
///
/// Calling a constructor checks each argument against the variant's field
/// [`Shape`]s and produces a [`TaggedValue`]. It is also a runtime [`Object`]
/// (callable, table-reflectable) so it can be installed as a value.
#[derive(Clone)]
pub struct VariantConstructor {
    adt: Symbol,
    variant: VariantDeclaration,
}

impl VariantConstructor {
    /// Builds a constructor for `variant` within ADT `adt`.
    pub fn new(adt: Symbol, variant: VariantDeclaration) -> Self {
        Self { adt, variant }
    }

    /// Returns the owning ADT name symbol.
    pub fn adt(&self) -> &Symbol {
        &self.adt
    }

    /// Returns the variant tag symbol.
    pub fn variant(&self) -> &Symbol {
        self.variant.symbol()
    }

    /// Returns the variant fields in declaration order.
    pub fn fields(&self) -> &[PatternField] {
        self.variant.fields()
    }

    /// Returns the kernel [`VariantShape`] that matches this variant.
    pub fn variant_shape(&self) -> VariantShape {
        VariantShape::new(
            self.adt.clone(),
            self.variant.symbol().clone(),
            self.variant.fields().to_vec(),
        )
    }

    /// Returns the variant matcher as a boxed kernel [`Shape`].
    pub fn shape(&self) -> Arc<dyn Shape> {
        Arc::new(self.variant_shape())
    }

    /// Builds a [`TaggedValue`] from the supplied field values.
    ///
    /// # Errors
    ///
    /// Returns an error if the arity is wrong or a field [`Shape`] rejects its
    /// value.
    pub fn construct(&self, cx: &mut Cx, fields: Vec<Value>) -> Result<Value> {
        if fields.len() != self.variant.fields().len() {
            return Err(Error::Eval(format!(
                "constructor {} expected {} fields, got {}",
                self.variant.symbol(),
                self.variant.fields().len(),
                fields.len()
            )));
        }
        for (field, value) in self.variant.fields().iter().zip(fields.iter()) {
            let matched = field.shape().check_value(cx, value.clone())?;
            if !matched.accepted {
                return Err(Error::Eval(format!(
                    "constructor {} rejected field {}: {}",
                    self.variant.symbol(),
                    field.name(),
                    diagnostic_summary(&matched.diagnostics)
                )));
            }
        }
        let fields = self
            .variant
            .fields()
            .iter()
            .map(|field| field.name().clone())
            .zip(fields)
            .collect();
        cx.factory().opaque(Arc::new(TaggedValue::new(
            self.adt.clone(),
            self.variant.symbol().clone(),
            fields,
        )))
    }

    /// Wraps the constructor itself as a callable runtime [`Value`].
    pub fn as_value(&self, cx: &mut Cx) -> Result<Value> {
        cx.factory().opaque(Arc::new(self.clone()))
    }
}

impl Object for VariantConstructor {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<constructor {}>", self.variant.symbol()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for VariantConstructor {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }

    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        cx.factory().table(vec![
            (Symbol::new("adt"), cx.factory().symbol(self.adt().clone())?),
            (
                Symbol::new("variant"),
                cx.factory().symbol(self.variant().clone())?,
            ),
            (
                Symbol::new("arity"),
                cx.factory().number_literal(
                    Symbol::qualified("numbers", "f64"),
                    self.fields().len().to_string(),
                )?,
            ),
        ])
    }
}

impl Callable for VariantConstructor {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        self.construct(cx, args.into_vec())
    }
}

#[sim_citizen_derive::non_citizen(
    reason = "dynamic ADT variant value; canonical data is the variant symbol and field table",
    kind = "marker",
    descriptor = "pattern/TaggedValue"
)]
/// A constructed ADT value: an ADT name, a variant tag, and named field values.
///
/// This is the runtime [`Object`] that [`VariantConstructor::construct`]
/// produces and that the [`AdtShape`]/[`VariantShape`] matchers recognize. Its
/// canonical data is the variant symbol plus the field table.
#[derive(Clone)]
pub struct TaggedValue {
    adt: Symbol,
    variant: Symbol,
    fields: Vec<(Symbol, Value)>,
    header: OnceLock<sim_kernel::ObjectHeader>,
}

impl TaggedValue {
    /// Builds a tagged value from its ADT name, variant tag, and named fields.
    pub fn new(adt: Symbol, variant: Symbol, fields: Vec<(Symbol, Value)>) -> Self {
        Self {
            adt,
            variant,
            fields,
            header: OnceLock::new(),
        }
    }

    /// Returns the owning ADT name symbol.
    pub fn adt(&self) -> &Symbol {
        &self.adt
    }

    /// Returns the variant tag symbol.
    pub fn variant(&self) -> &Symbol {
        &self.variant
    }

    /// Returns the named field values in construction order.
    pub fn fields(&self) -> &[(Symbol, Value)] {
        &self.fields
    }

    /// Returns the value of field `name`, or `None` if absent.
    pub fn field(&self, name: &Symbol) -> Option<&Value> {
        self.fields
            .iter()
            .find_map(|(field, value)| (field == name).then_some(value))
    }
}

impl Object for TaggedValue {
    fn header(&self) -> &sim_kernel::ObjectHeader {
        self.header.get_or_init(|| sim_kernel::ObjectHeader {
            id: sim_kernel::Ref::Symbol(self.variant.clone()),
            kind: Symbol::qualified("pattern", "tagged-value"),
            trust: sim_kernel::TrustLevel::HostInternal,
        })
    }

    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<{} {}>", self.adt, self.variant))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for TaggedValue {
    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        let fields = self
            .fields
            .iter()
            .map(|(name, value)| (name.clone(), value.clone()))
            .collect();
        let fields = cx.factory().table(fields)?;
        cx.factory().table(vec![
            (Symbol::new("adt"), cx.factory().symbol(self.adt.clone())?),
            (
                Symbol::new("variant"),
                cx.factory().symbol(self.variant.clone())?,
            ),
            (Symbol::new("fields"), fields),
        ])
    }

    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        let args = self
            .fields
            .iter()
            .map(|(_, value)| value.object().as_expr(cx))
            .collect::<Result<Vec<_>>>()?;
        Ok(Expr::Call {
            operator: Box::new(Expr::Symbol(self.variant.clone())),
            args,
        })
    }
}

/// Downcasts a runtime [`Value`] to a [`TaggedValue`], if it is one.
pub fn tagged_value(value: &Value) -> Option<&TaggedValue> {
    value.object().downcast_ref::<TaggedValue>()
}

fn diagnostic_summary(diagnostics: &[sim_kernel::Diagnostic]) -> String {
    diagnostics
        .first()
        .map(|diagnostic| diagnostic.message.clone())
        .unwrap_or_else(|| "field shape rejected value".to_owned())
}
