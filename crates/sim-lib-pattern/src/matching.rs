use std::sync::Arc;

use sim_kernel::{
    Cx, Diagnostic, Error, Expr, MatchScore, Result, Shape, ShapeBindings, ShapeMatch, Symbol,
    Value,
};

use crate::{AlgebraicDataType, VariantConstructor};

/// One arm of a match: a labelled pattern [`Shape`] plus optional coverage info.
///
/// The kernel defines the [`Shape`] match/binding contract; an arm wraps a
/// shape with the label reported on a hit and, optionally, the ADT variant it
/// covers so [`exhaustiveness_diagnostics`] can verify completeness.
#[derive(Clone)]
pub struct MatchArm {
    label: Symbol,
    shape: Arc<dyn Shape>,
    covered_variant: Option<Symbol>,
}

impl MatchArm {
    /// Builds an arm from a label and a checking [`Shape`], covering no variant.
    pub fn new(label: Symbol, shape: Arc<dyn Shape>) -> Self {
        Self {
            label,
            shape,
            covered_variant: None,
        }
    }

    /// Builds an arm matching `constructor`'s variant, recording its coverage.
    pub fn for_constructor(constructor: &VariantConstructor) -> Self {
        Self {
            label: constructor.variant().clone(),
            shape: constructor.shape(),
            covered_variant: Some(constructor.variant().clone()),
        }
    }

    /// Records the ADT variant this arm covers, for exhaustiveness checking.
    pub fn with_covered_variant(mut self, variant: Symbol) -> Self {
        self.covered_variant = Some(variant);
        self
    }

    /// Returns the label reported when this arm matches.
    pub fn label(&self) -> &Symbol {
        &self.label
    }

    /// Returns the kernel [`Shape`] this arm checks against.
    pub fn shape(&self) -> &Arc<dyn Shape> {
        &self.shape
    }

    /// Returns the ADT variant this arm covers, if any.
    pub fn covered_variant(&self) -> Option<&Symbol> {
        self.covered_variant.as_ref()
    }
}

/// The outcome of a successful [`match_value`]: which arm fired, its bindings,
/// and the match score.
#[derive(Clone, Debug)]
pub struct PatternMatch {
    arm_index: usize,
    label: Symbol,
    captures: ShapeBindings,
    score: MatchScore,
}

impl PatternMatch {
    /// Returns the index of the arm that matched.
    pub fn arm_index(&self) -> usize {
        self.arm_index
    }

    /// Returns the label of the arm that matched.
    pub fn label(&self) -> &Symbol {
        &self.label
    }

    /// Returns the kernel [`ShapeBindings`] captured by the matching arm.
    pub fn captures(&self) -> &ShapeBindings {
        &self.captures
    }

    /// Returns the kernel [`MatchScore`] the matching arm earned.
    pub fn score(&self) -> MatchScore {
        self.score
    }
}

/// Matches `value` against `arms` in order, returning the first that accepts.
///
/// The kernel defines the [`Shape`] match/binding contract; this runs each
/// arm's shape over the value and reports the first hit with its captured
/// [`ShapeBindings`].
///
/// # Errors
///
/// Returns an error if no arm accepts the value.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Symbol};
/// use sim_lib_pattern::{
///     AlgebraicDataType, MatchArm, VariantDeclaration, match_value,
/// };
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let maybe = AlgebraicDataType::new(
///     Symbol::qualified("adt", "Maybe"),
///     vec![
///         VariantDeclaration::nullary(Symbol::qualified("maybe", "Nothing")),
///         VariantDeclaration::nullary(Symbol::qualified("maybe", "Just")),
///     ],
/// )
/// .unwrap();
/// let nothing = maybe.constructor(&Symbol::qualified("maybe", "Nothing")).unwrap();
/// let just = maybe.constructor(&Symbol::qualified("maybe", "Just")).unwrap();
/// let value = just.construct(&mut cx, vec![]).unwrap();
///
/// let matched = match_value(
///     &mut cx,
///     value,
///     &[
///         MatchArm::for_constructor(&nothing),
///         MatchArm::for_constructor(&just),
///     ],
/// )
/// .unwrap();
/// assert_eq!(matched.arm_index(), 1);
/// assert_eq!(matched.label(), &Symbol::qualified("maybe", "Just"));
/// ```
pub fn match_value(cx: &mut Cx, value: Value, arms: &[MatchArm]) -> Result<PatternMatch> {
    let mut diagnostics = Vec::new();
    for (index, arm) in arms.iter().enumerate() {
        let matched = arm.shape().check_value(cx, value.clone())?;
        if matched.accepted {
            return Ok(PatternMatch {
                arm_index: index,
                label: arm.label().clone(),
                captures: matched.captures,
                score: matched.score,
            });
        }
        diagnostics.extend(matched.diagnostics);
    }
    Err(Error::Eval(format!(
        "no pattern arm matched: {}",
        diagnostic_summary(&diagnostics)
    )))
}

/// Checks `value` against a single `shape`, returning the kernel [`ShapeMatch`].
///
/// Thin pass-through to the kernel match contract, exposed as the pattern
/// organ's value-destructuring entry point.
///
/// # Errors
///
/// Propagates any error from the shape's value check.
pub fn destructure_value(cx: &mut Cx, value: Value, shape: &dyn Shape) -> Result<ShapeMatch> {
    shape.check_value(cx, value)
}

/// Checks `expr` against a single `shape`, returning the kernel [`ShapeMatch`].
///
/// The expression-side counterpart to [`destructure_value`], used to match and
/// bind unevaluated [`Expr`] forms.
///
/// # Errors
///
/// Propagates any error from the shape's expression check.
pub fn destructure_expr(cx: &mut Cx, expr: &Expr, shape: &dyn Shape) -> Result<ShapeMatch> {
    shape.check_expr(cx, expr)
}

/// Returns diagnostics for any `adt` variants not covered by `arms`.
///
/// Returns an empty vector when the arms cover every variant; otherwise a
/// single error diagnostic listing the missing variants.
pub fn exhaustiveness_diagnostics(adt: &AlgebraicDataType, arms: &[MatchArm]) -> Vec<Diagnostic> {
    let covered = arms
        .iter()
        .filter_map(MatchArm::covered_variant)
        .collect::<std::collections::BTreeSet<_>>();
    let missing = adt
        .variants()
        .filter(|variant| !covered.contains(variant.symbol()))
        .map(|variant| variant.symbol().to_string())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Vec::new();
    }
    let mut diagnostic = Diagnostic::error(format!(
        "non-exhaustive match for {}: missing {}",
        adt.symbol(),
        missing.join(", ")
    ));
    diagnostic.code = Some(Symbol::qualified("pattern", "non-exhaustive"));
    vec![diagnostic]
}

fn diagnostic_summary(diagnostics: &[Diagnostic]) -> String {
    diagnostics
        .first()
        .map(|diagnostic| diagnostic.message.clone())
        .unwrap_or_else(|| "all pattern shapes rejected the value".to_owned())
}
