use std::{collections::BTreeMap, error::Error, fmt};

use sim_kernel::{ShapeId, Symbol};

/// The ways in which a caller may address a parameter.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct CallMode {
    positional: bool,
    named: bool,
}

impl CallMode {
    /// The parameter is addressed only by declaration position.
    pub const POSITIONAL: Self = Self::new(true, false);
    /// The parameter is addressed only by name.
    pub const NAMED: Self = Self::new(false, true);
    /// The parameter may be addressed by position or name.
    pub const POSITIONAL_OR_NAMED: Self = Self::new(true, true);

    /// Constructs a call mode from its two independent addressing facets.
    ///
    /// A mode with neither facet is contradictory and is refused by
    /// [`FunctionPlan::new`].
    pub const fn new(positional: bool, named: bool) -> Self {
        Self { positional, named }
    }

    /// Whether positional addressing is admitted.
    pub const fn is_positional(self) -> bool {
        self.positional
    }

    /// Whether named addressing is admitted.
    pub const fn is_named(self) -> bool {
        self.named
    }
}

/// The declaration role of a parameter.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ParameterKind {
    /// A value must be supplied by the caller.
    Required,
    /// Guest policy may supply a default when the caller omits the value.
    Optional,
    /// Remaining arguments in the selected call-mode partition are collected.
    Remainder,
}

/// Stable declaration metadata for one parameter.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ParameterDescriptor {
    name: Symbol,
    kind: ParameterKind,
    call_mode: CallMode,
    shape: Option<ShapeId>,
}

impl ParameterDescriptor {
    /// Declares a parameter with an optional existing Shape identifier.
    pub fn new(
        name: Symbol,
        kind: ParameterKind,
        call_mode: CallMode,
        shape: Option<ShapeId>,
    ) -> Self {
        Self {
            name,
            kind,
            call_mode,
            shape,
        }
    }

    /// Returns the binding name.
    pub fn name(&self) -> &Symbol {
        &self.name
    }
    /// Returns the declaration role.
    pub const fn kind(&self) -> ParameterKind {
        self.kind
    }
    /// Returns the admitted addressing mode.
    pub const fn call_mode(&self) -> CallMode {
        self.call_mode
    }
    /// Returns the stable Shape identifier used for browsing, when declared.
    pub const fn shape(&self) -> Option<ShapeId> {
        self.shape
    }
}

/// Stable metadata for one lexical capture slot.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CaptureDescriptor {
    name: Symbol,
    shape: Option<ShapeId>,
}

impl CaptureDescriptor {
    /// Declares a capture slot with an optional existing Shape identifier.
    pub fn new(name: Symbol, shape: Option<ShapeId>) -> Self {
        Self { name, shape }
    }
    /// Returns the capture binding name.
    pub fn name(&self) -> &Symbol {
        &self.name
    }
    /// Returns its stable Shape identifier, when declared.
    pub const fn shape(&self) -> Option<ShapeId> {
        self.shape
    }
}

/// Canonical, inert projection of a function's browsable Shape metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BrowseProjection {
    parameters: Vec<(Symbol, Option<ShapeId>)>,
    result: Option<ShapeId>,
}

impl BrowseProjection {
    /// Returns parameter names and Shape identifiers in declaration order.
    pub fn parameters(&self) -> &[(Symbol, Option<ShapeId>)] {
        &self.parameters
    }
    /// Returns the declared result Shape identifier.
    pub const fn result(&self) -> Option<ShapeId> {
        self.result
    }
}

/// A construction failure for an immutable function plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlanError {
    message: String,
}

impl PlanError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for PlanError {}

/// Immutable declaration metadata shared by guest function implementations.
///
/// Equality is plan identity: two independently built plans compare equal when
/// their stable display symbol and complete declarations are equal.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct FunctionPlan {
    display_identity: Symbol,
    parameters: Vec<ParameterDescriptor>,
    captures: Vec<CaptureDescriptor>,
    result_shape: Option<ShapeId>,
}

impl FunctionPlan {
    /// Validates and freezes one declaration.
    pub fn new(
        display_identity: Symbol,
        parameters: Vec<ParameterDescriptor>,
        captures: Vec<CaptureDescriptor>,
        result_shape: Option<ShapeId>,
    ) -> Result<Self, PlanError> {
        validate_parameters(&parameters)?;
        validate_captures(&captures)?;
        Ok(Self {
            display_identity,
            parameters,
            captures,
            result_shape,
        })
    }

    /// Returns the stable human-facing identity of this declaration.
    pub fn display_identity(&self) -> &Symbol {
        &self.display_identity
    }
    /// Returns parameter descriptors in declaration order.
    pub fn parameters(&self) -> &[ParameterDescriptor] {
        &self.parameters
    }
    /// Returns capture slots in stable declaration order.
    pub fn captures(&self) -> &[CaptureDescriptor] {
        &self.captures
    }
    /// Returns the declared result Shape identifier.
    pub const fn result_shape(&self) -> Option<ShapeId> {
        self.result_shape
    }

    /// Builds the canonical browse projection without resolving or executing a Shape.
    pub fn browse(&self) -> BrowseProjection {
        BrowseProjection {
            parameters: self
                .parameters
                .iter()
                .map(|p| (p.name.clone(), p.shape))
                .collect(),
            result: self.result_shape,
        }
    }
}

fn validate_parameters(parameters: &[ParameterDescriptor]) -> Result<(), PlanError> {
    let mut names = BTreeMap::new();
    let mut positional_remainder: Option<&Symbol> = None;
    for parameter in parameters {
        if let Some(first) = names.insert(parameter.name.clone(), parameter.name.clone()) {
            return Err(PlanError::new(format!(
                "duplicate parameter names {first} and {}",
                parameter.name
            )));
        }
        if !parameter.call_mode.positional && !parameter.call_mode.named {
            return Err(PlanError::new(format!(
                "parameter {} has contradictory call modes",
                parameter.name
            )));
        }
        if let Some(remainder) = positional_remainder
            && parameter.kind == ParameterKind::Required
            && parameter.call_mode.positional
        {
            return Err(PlanError::new(format!(
                "positional remainder {remainder} cannot precede required parameter {}",
                parameter.name
            )));
        }
        if parameter.kind == ParameterKind::Remainder {
            if parameter.call_mode == CallMode::POSITIONAL_OR_NAMED {
                return Err(PlanError::new(format!(
                    "remainder parameter {} has contradictory call modes",
                    parameter.name
                )));
            }
            if parameter.call_mode.positional {
                if let Some(first) = positional_remainder {
                    return Err(PlanError::new(format!(
                        "positional remainders {first} and {} conflict",
                        parameter.name
                    )));
                }
                positional_remainder = Some(&parameter.name);
            }
        }
    }
    Ok(())
}

fn validate_captures(captures: &[CaptureDescriptor]) -> Result<(), PlanError> {
    let mut names = BTreeMap::new();
    for capture in captures {
        if let Some(first) = names.insert(capture.name.clone(), capture.name.clone()) {
            return Err(PlanError::new(format!(
                "duplicate capture names {first} and {}",
                capture.name
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parameter(name: &str, kind: ParameterKind, mode: CallMode) -> ParameterDescriptor {
        ParameterDescriptor::new(Symbol::new(name), kind, mode, None)
    }

    #[test]
    fn remainder_before_required_names_both_parameters() {
        let error = FunctionPlan::new(
            Symbol::new("example"),
            vec![
                parameter("rest", ParameterKind::Remainder, CallMode::POSITIONAL),
                parameter("needed", ParameterKind::Required, CallMode::POSITIONAL),
            ],
            vec![],
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("rest"));
        assert!(error.to_string().contains("needed"));
    }

    #[test]
    fn equal_declarations_have_equal_identity() {
        let build = || {
            FunctionPlan::new(
                Symbol::qualified("guest", "work"),
                vec![parameter(
                    "value",
                    ParameterKind::Required,
                    CallMode::POSITIONAL_OR_NAMED,
                )],
                vec![CaptureDescriptor::new(
                    Symbol::new("scope"),
                    Some(ShapeId(7)),
                )],
                Some(ShapeId(9)),
            )
            .unwrap()
        };
        assert_eq!(build(), build());
    }

    #[test]
    fn construction_rejects_duplicates_and_contradictory_modes() {
        let duplicate = FunctionPlan::new(
            Symbol::new("duplicate"),
            vec![
                parameter("same", ParameterKind::Required, CallMode::NAMED),
                parameter("same", ParameterKind::Optional, CallMode::NAMED),
            ],
            vec![],
            None,
        )
        .unwrap_err();
        assert!(duplicate.to_string().contains("same"));

        let contradictory = FunctionPlan::new(
            Symbol::new("contradictory"),
            vec![parameter(
                "lost",
                ParameterKind::Required,
                CallMode::new(false, false),
            )],
            vec![],
            None,
        )
        .unwrap_err();
        assert!(contradictory.to_string().contains("lost"));
    }

    #[test]
    fn browse_projection_preserves_shape_identifiers() {
        let plan = FunctionPlan::new(
            Symbol::new("browse"),
            vec![ParameterDescriptor::new(
                Symbol::new("input"),
                ParameterKind::Required,
                CallMode::POSITIONAL,
                Some(ShapeId(3)),
            )],
            vec![],
            Some(ShapeId(4)),
        )
        .unwrap();
        assert_eq!(
            plan.browse().parameters(),
            &[(Symbol::new("input"), Some(ShapeId(3)))]
        );
        assert_eq!(plan.browse().result(), Some(ShapeId(4)));
    }
}
