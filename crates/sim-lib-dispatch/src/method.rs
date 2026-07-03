use std::{cmp::Ordering, sync::Arc};

use sim_kernel::{
    CapabilityName, Cx, Error, HintMetadata, MatchScore, Result, Shape, Symbol, Value,
};

/// The executable body of a [`DispatchMethod`].
///
/// A shared closure invoked with the call [`Cx`] and the matched arguments,
/// returning the method's result [`Value`].
pub type MethodBody = Arc<dyn Fn(&mut Cx, &[Value]) -> Result<Value> + Send + Sync>;

/// Role of a method within a generic function's combination.
///
/// Roles order the execution plan: `Around` wraps the call, `Before` and
/// `After` run for effect around the single applicable `Primary`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MethodRole {
    /// Wraps the whole call; most specific runs outermost.
    Around,
    /// Runs for effect before the primary method; most specific first.
    Before,
    /// Provides the call's result; exactly one applies per call.
    Primary,
    /// Runs for effect after the primary method; least specific first.
    After,
}

impl MethodRole {
    /// Returns the qualified `method-role:<role>` symbol naming this role.
    pub fn as_symbol(self) -> Symbol {
        match self {
            Self::Around => Symbol::qualified("method-role", "around"),
            Self::Before => Symbol::qualified("method-role", "before"),
            Self::Primary => Symbol::qualified("method-role", "primary"),
            Self::After => Symbol::qualified("method-role", "after"),
        }
    }

    /// Returns the role's ordering rank within the combination plan.
    ///
    /// Lower ranks run earlier: around (0), before (1), primary (2), after (3).
    pub fn combination_rank(self) -> u8 {
        match self {
            Self::Around => 0,
            Self::Before => 1,
            Self::Primary => 2,
            Self::After => 3,
        }
    }
}

/// A single method belonging to a [`GenericFunction`](crate::GenericFunction).
///
/// Pairs a parameter-shape signature with an executable [`MethodBody`] and a
/// [`MethodRole`]; methods are matched against call arguments through the
/// kernel [`Shape`] protocol.
#[derive(Clone)]
pub struct DispatchMethod {
    id: Symbol,
    role: MethodRole,
    parameter_shapes: Vec<Arc<dyn Shape>>,
    body: MethodBody,
    hints: Vec<HintMetadata>,
}

impl DispatchMethod {
    /// Builds a method from its id, role, parameter shapes, and body.
    pub fn new(
        id: Symbol,
        role: MethodRole,
        parameter_shapes: Vec<Arc<dyn Shape>>,
        body: MethodBody,
    ) -> Self {
        Self {
            id,
            role,
            parameter_shapes,
            body,
            hints: Vec::new(),
        }
    }

    /// Returns the symbol identifying this method.
    pub fn id(&self) -> &Symbol {
        &self.id
    }

    /// Returns the method's role in the combination.
    pub fn role(&self) -> MethodRole {
        self.role
    }

    /// Returns the method's arity (its number of parameter shapes).
    pub fn arity(&self) -> usize {
        self.parameter_shapes.len()
    }

    /// Returns the parameter shapes this method matches against, in order.
    pub fn parameter_shapes(&self) -> &[Arc<dyn Shape>] {
        &self.parameter_shapes
    }

    /// Returns the operation hints attached to this method.
    pub fn hints(&self) -> &[HintMetadata] {
        &self.hints
    }

    /// Adds an operation hint to this method.
    pub fn with_hint(mut self, hint: HintMetadata) -> Self {
        self.hints.push(hint);
        self
    }

    /// Adds a hint describing one method argument.
    pub fn with_argument_hint(self, argument: Symbol, detail: impl Into<String>) -> Self {
        let title = format!("argument {argument}");
        self.with_hint(
            HintMetadata::new(Symbol::qualified("runtime-hint", "argument"), title)
                .with_detail(detail)
                .with_tag(Symbol::qualified("runtime", "argument"))
                .with_argument(argument),
        )
    }

    /// Adds a hint describing a capability requirement.
    pub fn with_capability_requirement(self, capability: CapabilityName) -> Self {
        self.with_hint(
            HintMetadata::new(
                Symbol::qualified("runtime-hint", "capability"),
                format!("requires {capability}"),
            )
            .with_tag(Symbol::qualified("runtime", "capability"))
            .with_capability(capability),
        )
    }

    /// Adds a hint describing a codec-safe form.
    pub fn with_codec_safe_form(self, form: Symbol) -> Self {
        self.with_hint(
            HintMetadata::new(
                Symbol::qualified("runtime-hint", "codec-form"),
                format!("codec form {form}"),
            )
            .with_tag(Symbol::qualified("runtime", "codec"))
            .with_codec_form(form),
        )
    }

    /// Adds a runnable or displayable operation example.
    pub fn with_example(self, example: impl Into<String>) -> Self {
        self.with_hint(
            HintMetadata::new(
                Symbol::qualified("runtime-hint", "example"),
                "operation example",
            )
            .with_tag(Symbol::qualified("runtime", "example"))
            .with_example(example),
        )
    }

    /// Tests the method against `args`, returning its specificity if applicable.
    ///
    /// Returns `None` on arity mismatch or if any parameter shape rejects its
    /// argument; otherwise returns the accumulated [`MethodSpecificity`].
    pub fn match_args(&self, cx: &mut Cx, args: &[Value]) -> Result<Option<MethodSpecificity>> {
        if args.len() != self.parameter_shapes.len() {
            return Ok(None);
        }

        let mut total = MatchScore::exact(0);
        let mut argument_scores = Vec::with_capacity(args.len());
        for (shape, arg) in self.parameter_shapes.iter().zip(args.iter()) {
            let matched = shape.check_value(cx, arg.clone())?;
            if !matched.accepted {
                return Ok(None);
            }
            total += matched.score;
            argument_scores.push(matched.score);
        }

        Ok(Some(MethodSpecificity::new(
            self.id.clone(),
            self.role,
            total,
            argument_scores,
        )))
    }

    /// Runs the method body against `args`, returning its result.
    pub fn invoke(&self, cx: &mut Cx, args: &[Value]) -> Result<Value> {
        (self.body)(cx, args)
    }
}

/// How specifically a method matched a particular call.
///
/// Carries the per-argument [`MatchScore`]s plus their total; comparing two
/// specificities (see [`compare_specificity`]) orders methods most-specific
/// first for applicable-method selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MethodSpecificity {
    method: Symbol,
    role: MethodRole,
    score: MatchScore,
    argument_scores: Vec<MatchScore>,
}

impl MethodSpecificity {
    /// Builds a specificity record for a matched method.
    pub fn new(
        method: Symbol,
        role: MethodRole,
        score: MatchScore,
        argument_scores: Vec<MatchScore>,
    ) -> Self {
        Self {
            method,
            role,
            score,
            argument_scores,
        }
    }

    /// Returns the symbol identifying the matched method.
    pub fn method(&self) -> &Symbol {
        &self.method
    }

    /// Returns the matched method's role in the combination.
    pub fn role(&self) -> MethodRole {
        self.role
    }

    /// Returns the total match score across all arguments.
    pub fn score(&self) -> MatchScore {
        self.score
    }

    /// Returns the per-argument match scores, in argument order.
    pub fn argument_scores(&self) -> &[MatchScore] {
        &self.argument_scores
    }
}

/// Orders two specificities from least to most specific.
///
/// Compares argument scores left to right, breaking ties on the total score;
/// the dispatch organ uses this to rank applicable methods.
pub fn compare_specificity(left: &MethodSpecificity, right: &MethodSpecificity) -> Ordering {
    left.argument_scores
        .cmp(&right.argument_scores)
        .then_with(|| left.score.cmp(&right.score))
}

pub(crate) fn ambiguous_primary_error(name: &Symbol, left: &Symbol, right: &Symbol) -> Error {
    Error::Eval(format!(
        "generic function {name} has ambiguous primary methods {left} and {right}"
    ))
}
