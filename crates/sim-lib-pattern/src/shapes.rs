use sim_kernel::{
    Cx, Expr, MatchScore, Result, Shape, ShapeBindings, ShapeDoc, ShapeMatch, Symbol, Value,
    shape_is_subshape_of,
};

use crate::{PatternField, tagged_value};

/// A kernel [`Shape`] that matches any [`TaggedValue`](crate::TaggedValue) of a
/// given ADT.
///
/// The kernel defines the [`Shape`] match/binding protocol; `AdtShape` is the
/// pattern organ's concrete implementation that accepts a value when one of the
/// ADT's [`VariantShape`]s accepts it, binding that variant's captures.
pub struct AdtShape {
    adt: Symbol,
    variants: Vec<VariantShape>,
}

impl AdtShape {
    /// Builds an ADT shape from its name and per-variant shapes.
    pub fn new(adt: Symbol, variants: Vec<VariantShape>) -> Self {
        Self { adt, variants }
    }

    /// Returns the ADT name symbol.
    pub fn adt(&self) -> &Symbol {
        &self.adt
    }

    /// Returns the per-variant shapes this ADT shape dispatches over.
    pub fn variants(&self) -> &[VariantShape] {
        &self.variants
    }
}

impl Shape for AdtShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(Symbol::qualified("pattern-adt", self.adt.to_string()))
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let Some(tagged) = tagged_value(&value) else {
            return Ok(ShapeMatch::reject("expected tagged ADT value"));
        };
        if tagged.adt() != &self.adt {
            return Ok(ShapeMatch::reject(format!(
                "expected ADT {}, got {}",
                self.adt,
                tagged.adt()
            )));
        }
        let mut diagnostics = Vec::new();
        for variant in &self.variants {
            let matched = variant.check_value(cx, value.clone())?;
            if matched.accepted {
                return Ok(matched);
            }
            diagnostics.extend(matched.diagnostics);
        }
        Ok(ShapeMatch {
            accepted: false,
            captures: ShapeBindings::new(),
            score: MatchScore::reject(),
            diagnostics,
        })
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let mut diagnostics = Vec::new();
        for variant in &self.variants {
            let matched = variant.check_expr(cx, expr)?;
            if matched.accepted {
                return Ok(matched);
            }
            diagnostics.extend(matched.diagnostics);
        }
        Ok(ShapeMatch {
            accepted: false,
            captures: ShapeBindings::new(),
            score: MatchScore::reject(),
            diagnostics,
        })
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new(format!("ADT {}", self.adt)))
    }
}

/// A kernel [`Shape`] that matches one specific ADT variant by tag and fields.
///
/// `VariantShape` accepts a [`TaggedValue`](crate::TaggedValue) whose ADT and
/// variant match and whose fields each pass their [`PatternField`] shape,
/// accumulating their captures. It also matches the equivalent constructor
/// [`Expr`], and reports subshape relationships against sibling variants and
/// the enclosing [`AdtShape`].
#[derive(Clone)]
pub struct VariantShape {
    adt: Symbol,
    variant: Symbol,
    fields: Vec<PatternField>,
}

impl VariantShape {
    /// Builds a variant shape from its ADT name, variant tag, and fields.
    pub fn new(adt: Symbol, variant: Symbol, fields: Vec<PatternField>) -> Self {
        Self {
            adt,
            variant,
            fields,
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

    /// Returns the variant fields in declaration order.
    pub fn fields(&self) -> &[PatternField] {
        &self.fields
    }

    fn check_field_values(&self, cx: &mut Cx, fields: &[(Symbol, Value)]) -> Result<ShapeMatch> {
        if fields.len() != self.fields.len() {
            return Ok(ShapeMatch::reject(format!(
                "variant {} expected {} fields, got {}",
                self.variant,
                self.fields.len(),
                fields.len()
            )));
        }

        let mut out = ShapeMatch::accept(MatchScore::exact(30));
        for (field, (actual_name, value)) in self.fields.iter().zip(fields.iter()) {
            if field.name() != actual_name {
                return Ok(ShapeMatch::reject(format!(
                    "expected field {}, got {}",
                    field.name(),
                    actual_name
                )));
            }
            let matched = field.shape().check_value(cx, value.clone())?;
            if !matched.accepted {
                return Ok(matched);
            }
            out.captures.extend(matched.captures);
            out.score += matched.score;
        }
        Ok(out)
    }

    fn check_field_exprs(&self, cx: &mut Cx, args: &[Expr]) -> Result<ShapeMatch> {
        if args.len() != self.fields.len() {
            return Ok(ShapeMatch::reject(format!(
                "variant {} expected {} fields, got {}",
                self.variant,
                self.fields.len(),
                args.len()
            )));
        }

        let mut out = ShapeMatch::accept(MatchScore::exact(25));
        for (field, expr) in self.fields.iter().zip(args.iter()) {
            let matched = field.shape().check_expr(cx, expr)?;
            if !matched.accepted {
                return Ok(matched);
            }
            out.captures.extend(matched.captures);
            out.score += matched.score;
        }
        Ok(out)
    }
}

impl Shape for VariantShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(Symbol::qualified(
            "pattern-variant",
            self.variant.to_string(),
        ))
    }

    fn is_subshape_of(&self, cx: &mut Cx, parent: &dyn Shape) -> Result<Option<bool>> {
        if let Some(parent) = parent.as_any().downcast_ref::<Self>() {
            return Ok(Some(
                self.adt == parent.adt && self.variant == parent.variant,
            ));
        }
        if let Some(parent) = parent.as_any().downcast_ref::<AdtShape>() {
            return Ok(Some(self.adt == parent.adt));
        }
        shape_is_subshape_of(cx, self.fields_parent().as_ref(), parent).map(Some)
    }

    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let Some(tagged) = tagged_value(&value) else {
            return Ok(ShapeMatch::reject("expected tagged ADT value"));
        };
        if tagged.adt() != &self.adt {
            return Ok(ShapeMatch::reject(format!(
                "expected ADT {}, got {}",
                self.adt,
                tagged.adt()
            )));
        }
        if tagged.variant() != &self.variant {
            return Ok(ShapeMatch::reject(format!(
                "expected variant {}, got {}",
                self.variant,
                tagged.variant()
            )));
        }
        self.check_field_values(cx, tagged.fields())
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let (operator, args) = match expr {
            Expr::Call { operator, args } => (operator.as_ref(), args.as_slice()),
            Expr::List(items) if !items.is_empty() => (&items[0], &items[1..]),
            _ => {
                return Ok(ShapeMatch::reject(
                    "expected variant constructor expression",
                ));
            }
        };
        let Expr::Symbol(symbol) = operator else {
            return Ok(ShapeMatch::reject("expected symbolic variant constructor"));
        };
        if symbol != &self.variant {
            return Ok(ShapeMatch::reject(format!(
                "expected variant {}, got {}",
                self.variant, symbol
            )));
        }
        self.check_field_exprs(cx, args)
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new(format!("variant {}", self.variant)))
    }
}

impl VariantShape {
    fn fields_parent(&self) -> std::sync::Arc<dyn Shape> {
        std::sync::Arc::new(AdtShape::new(self.adt.clone(), vec![self.clone()]))
    }
}
