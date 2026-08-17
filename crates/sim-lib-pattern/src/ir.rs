//! Validated, dialect-neutral pattern intermediate representation.

use crate::SymbolDomain;
use core::fmt;
use core::marker::PhantomData;
use std::collections::{BTreeMap, BTreeSet};

/// Stable identifier for a tagged capture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CaptureId(pub u32);

/// Stable identifier for a named assertion definition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssertionId(pub u32);

/// A zero-width subject anchor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    /// Start of the subject.
    SubjectStart,
    /// End of the subject.
    SubjectEnd,
}

/// Valid bounds for a repetition node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepeatBounds {
    min: usize,
    max: Option<usize>,
}

impl RepeatBounds {
    /// Creates repetition bounds, rejecting a finite maximum below `min`.
    ///
    /// ```
    /// use sim_lib_pattern::RepeatBounds;
    ///
    /// let error = RepeatBounds::new(4, Some(3)).unwrap_err();
    /// assert_eq!(
    ///     error.to_string(),
    ///     "invalid repeat bounds: minimum 4 exceeds maximum 3"
    /// );
    /// ```
    pub fn new(min: usize, max: Option<usize>) -> Result<Self, IrError> {
        if let Some(max) = max
            && min > max
        {
            return Err(IrError::InvalidRepeatBounds { min, max });
        }
        Ok(Self { min, max })
    }

    /// Returns the minimum number of matches.
    pub const fn min(self) -> usize {
        self.min
    }

    /// Returns the maximum number of matches, or `None` when unbounded.
    pub const fn max(self) -> Option<usize> {
        self.max
    }
}

/// One structured pattern expression.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrNode<S, E> {
    /// Match one exact symbol.
    Symbol(S),
    /// Match any one symbol.
    Any,
    /// Match every child in order.
    Concat(Vec<Self>),
    /// Try each child as an alternative.
    Alternation(Vec<Self>),
    /// Repeat a child within validated bounds.
    Repeat {
        /// Expression being repeated.
        node: Box<Self>,
        /// Validated repetition bounds.
        bounds: RepeatBounds,
        /// Whether longer matches are preferred.
        greedy: bool,
    },
    /// Preserve explicit grouping from the source dialect.
    Group(Box<Self>),
    /// Record the child's subject span under a stable tag.
    Capture {
        /// Unique capture tag within the IR.
        id: CaptureId,
        /// Captured expression.
        node: Box<Self>,
    },
    /// Test a zero-width subject boundary.
    Anchor(Anchor),
    /// Evaluate a separately declared zero-width assertion.
    Assertion(AssertionId),
    /// A dialect-specific operation explicitly admitted by the target engine.
    Extension(E),
}

/// Target-engine policy for dialect extension nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnginePolicy<E> {
    admitted_extensions: BTreeSet<E>,
}

impl<E: Ord> EnginePolicy<E> {
    /// Creates a policy from exactly the extension operations an engine admits.
    pub fn new(admitted_extensions: impl IntoIterator<Item = E>) -> Self {
        Self {
            admitted_extensions: admitted_extensions.into_iter().collect(),
        }
    }
}

/// A fully validated pattern IR tied to one symbol and offset domain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PatternIr<D: SymbolDomain, E> {
    root: IrNode<D::Symbol, E>,
    assertions: BTreeMap<AssertionId, IrNode<D::Symbol, E>>,
    domain: PhantomData<fn() -> D>,
}

impl<D, E> PatternIr<D, E>
where
    D: SymbolDomain,
    E: Clone + fmt::Debug + Ord,
{
    /// Validates and creates an IR for a target engine.
    pub fn new(
        root: IrNode<D::Symbol, E>,
        assertions: BTreeMap<AssertionId, IrNode<D::Symbol, E>>,
        policy: &EnginePolicy<E>,
    ) -> Result<Self, IrError> {
        let mut captures = BTreeSet::new();
        validate_node(&root, &assertions, policy, &mut captures)?;
        for definition in assertions.values() {
            validate_node(definition, &assertions, policy, &mut captures)?;
        }
        validate_assertion_cycles(&root, &assertions, &mut Vec::new())?;
        for (id, definition) in &assertions {
            validate_assertion_cycles(definition, &assertions, &mut vec![*id])?;
        }
        Ok(Self {
            root,
            assertions,
            domain: PhantomData,
        })
    }

    /// Returns the root expression.
    pub fn root(&self) -> &IrNode<D::Symbol, E> {
        &self.root
    }

    /// Returns the validated assertion definitions.
    pub fn assertions(&self) -> &BTreeMap<AssertionId, IrNode<D::Symbol, E>> {
        &self.assertions
    }
}

/// A construction failure for pattern IR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrError {
    /// A repeat's minimum exceeds its maximum.
    InvalidRepeatBounds {
        /// Requested minimum.
        min: usize,
        /// Requested finite maximum.
        max: usize,
    },
    /// A capture identifier occurs more than once.
    DuplicateCapture(CaptureId),
    /// An assertion refers to a definition that was not supplied.
    MissingAssertion(AssertionId),
    /// Assertion references form a cycle.
    AssertionCycle(Vec<AssertionId>),
    /// An extension is unavailable on the selected target engine.
    UnsupportedExtension(String),
}

impl fmt::Display for IrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRepeatBounds { min, max } => {
                write!(
                    f,
                    "invalid repeat bounds: minimum {min} exceeds maximum {max}"
                )
            }
            Self::DuplicateCapture(id) => write!(f, "duplicate capture id {}", id.0),
            Self::MissingAssertion(id) => write!(f, "missing assertion id {}", id.0),
            Self::AssertionCycle(path) => write!(f, "assertion cycle: {path:?}"),
            Self::UnsupportedExtension(extension) => {
                write!(f, "target engine does not admit extension {extension}")
            }
        }
    }
}

impl std::error::Error for IrError {}

fn validate_node<S, E>(
    node: &IrNode<S, E>,
    assertions: &BTreeMap<AssertionId, IrNode<S, E>>,
    policy: &EnginePolicy<E>,
    captures: &mut BTreeSet<CaptureId>,
) -> Result<(), IrError>
where
    E: fmt::Debug + Ord,
{
    match node {
        IrNode::Concat(nodes) | IrNode::Alternation(nodes) => {
            for node in nodes {
                validate_node(node, assertions, policy, captures)?;
            }
        }
        IrNode::Repeat { node, .. } | IrNode::Group(node) => {
            validate_node(node, assertions, policy, captures)?;
        }
        IrNode::Capture { id, node } => {
            if !captures.insert(*id) {
                return Err(IrError::DuplicateCapture(*id));
            }
            validate_node(node, assertions, policy, captures)?;
        }
        IrNode::Assertion(id) => {
            assertions.get(id).ok_or(IrError::MissingAssertion(*id))?;
        }
        IrNode::Extension(extension) if !policy.admitted_extensions.contains(extension) => {
            return Err(IrError::UnsupportedExtension(format!("{extension:?}")));
        }
        IrNode::Symbol(_) | IrNode::Any | IrNode::Anchor(_) | IrNode::Extension(_) => {}
    }
    Ok(())
}

fn validate_assertion_cycles<S, E>(
    node: &IrNode<S, E>,
    assertions: &BTreeMap<AssertionId, IrNode<S, E>>,
    path: &mut Vec<AssertionId>,
) -> Result<(), IrError> {
    match node {
        IrNode::Concat(nodes) | IrNode::Alternation(nodes) => {
            for node in nodes {
                validate_assertion_cycles(node, assertions, path)?;
            }
        }
        IrNode::Repeat { node, .. } | IrNode::Group(node) | IrNode::Capture { node, .. } => {
            validate_assertion_cycles(node, assertions, path)?;
        }
        IrNode::Assertion(id) => {
            if let Some(cycle_start) = path.iter().position(|seen| seen == id) {
                let mut cycle = path[cycle_start..].to_vec();
                cycle.push(*id);
                return Err(IrError::AssertionCycle(cycle));
            }
            let definition = assertions.get(id).ok_or(IrError::MissingAssertion(*id))?;
            path.push(*id);
            let result = validate_assertion_cycles(definition, assertions, path);
            path.pop();
            result?;
        }
        IrNode::Symbol(_) | IrNode::Any | IrNode::Anchor(_) | IrNode::Extension(_) => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ByteDomain;

    #[test]
    fn invalid_repeat_names_both_bounds() {
        let error = RepeatBounds::new(4, Some(3)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid repeat bounds: minimum 4 exceeds maximum 3"
        );
    }

    #[test]
    fn rejects_duplicate_capture_ids() {
        let capture = |symbol| IrNode::Capture {
            id: CaptureId(7),
            node: Box::new(IrNode::Symbol(symbol)),
        };
        let root = IrNode::Concat(vec![capture(b'a'), capture(b'b')]);
        let error =
            PatternIr::<ByteDomain, &str>::new(root, BTreeMap::new(), &EnginePolicy::new([]))
                .unwrap_err();
        assert_eq!(error, IrError::DuplicateCapture(CaptureId(7)));
    }

    #[test]
    fn rejects_assertion_cycles() {
        let assertions = BTreeMap::from([
            (AssertionId(1), IrNode::Assertion(AssertionId(2))),
            (AssertionId(2), IrNode::Assertion(AssertionId(1))),
        ]);
        let error = PatternIr::<ByteDomain, &str>::new(
            IrNode::Assertion(AssertionId(1)),
            assertions,
            &EnginePolicy::new([]),
        )
        .unwrap_err();
        assert_eq!(
            error,
            IrError::AssertionCycle(vec![AssertionId(1), AssertionId(2), AssertionId(1)])
        );
    }

    #[test]
    fn target_controls_dialect_extensions() {
        let denied = PatternIr::<ByteDomain, &str>::new(
            IrNode::Extension("backreference"),
            BTreeMap::new(),
            &EnginePolicy::new([]),
        );
        assert!(matches!(denied, Err(IrError::UnsupportedExtension(_))));

        let admitted = PatternIr::<ByteDomain, &str>::new(
            IrNode::Extension("backreference"),
            BTreeMap::new(),
            &EnginePolicy::new(["backreference"]),
        );
        assert!(admitted.is_ok());
    }
}
