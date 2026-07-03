use std::cmp::Ordering;

use sim_kernel::{Cx, Diagnostic, Error, HintMetadata, Result, Symbol, Value};

use crate::{
    DispatchMethod, MethodRole, MethodSpecificity,
    method::{ambiguous_primary_error, compare_specificity},
};

/// A generic function under its multimethod alias.
///
/// `Multimethod` and [`GenericFunction`] are the same dispatch entity; the
/// alias lets multimethod-flavored language profiles name it familiarly.
pub type Multimethod = GenericFunction;

/// A named generic function: a set of [`DispatchMethod`]s sharing one name.
///
/// This is the dispatch organ's central object. Methods are added with
/// [`add_method`](GenericFunction::add_method); a call selects the single most
/// specific applicable primary method (plus around/before/after methods) by
/// matching arguments through the kernel `Shape` protocol. The kernel defines
/// the callable/operation contracts; this type supplies the concrete dispatch.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, ExprKind, NoopEvalPolicy, Shape, Symbol};
/// use sim_lib_dispatch::{DispatchMethod, GenericFunction, MethodRole};
/// use sim_shape::{AnyShape, ExprKindShape};
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let mut generic = GenericFunction::new(Symbol::qualified("demo", "describe"));
///
/// // A broad fallback and a more specific string method.
/// generic.add_method(DispatchMethod::new(
///     Symbol::qualified("method", "any"),
///     MethodRole::Primary,
///     vec![Arc::new(AnyShape) as Arc<dyn Shape>],
///     Arc::new(|cx: &mut Cx, _| cx.factory().string("any".to_owned())),
/// )).unwrap();
/// generic.add_method(DispatchMethod::new(
///     Symbol::qualified("method", "string"),
///     MethodRole::Primary,
///     vec![Arc::new(ExprKindShape::new(ExprKind::String)) as Arc<dyn Shape>],
///     Arc::new(|cx: &mut Cx, _| cx.factory().string("string".to_owned())),
/// )).unwrap();
///
/// let args = [cx.factory().string("hi".to_owned()).unwrap()];
/// // The string method is more specific, so it is selected.
/// let selected = generic.select_primary(&mut cx, &args).unwrap();
/// assert_eq!(selected.method(), &Symbol::qualified("method", "string"));
/// ```
pub struct GenericFunction {
    name: Symbol,
    methods: Vec<DispatchMethod>,
}

impl GenericFunction {
    /// Creates an empty generic function with the given name.
    pub fn new(name: Symbol) -> Self {
        Self {
            name,
            methods: Vec::new(),
        }
    }

    /// Returns the generic function's name.
    pub fn name(&self) -> &Symbol {
        &self.name
    }

    /// Returns the methods added to this generic function, in insertion order.
    pub fn methods(&self) -> &[DispatchMethod] {
        &self.methods
    }

    /// Returns agent-consumable operation hints from the generic and methods.
    pub fn operation_hints(&self) -> Vec<HintMetadata> {
        let mut hints = vec![
            HintMetadata::new(
                Symbol::qualified("runtime-hint", "operation"),
                format!("operation {}", self.name),
            )
            .with_tag(Symbol::qualified("runtime", "operation"))
            .with_argument(self.name.clone()),
        ];
        for method in &self.methods {
            hints.push(
                HintMetadata::new(
                    Symbol::qualified("runtime-hint", "method"),
                    format!("method {}", method.id()),
                )
                .with_detail(format!(
                    "{} method with {} argument shapes",
                    method.role().as_symbol(),
                    method.arity()
                ))
                .with_tag(Symbol::qualified("runtime", "method"))
                .with_argument(method.id().clone()),
            );
            hints.extend(method.hints().iter().cloned());
        }
        hints
    }

    /// Adds a method, erroring if one with the same id, role, and arity exists.
    pub fn add_method(&mut self, method: DispatchMethod) -> Result<()> {
        if let Some(existing) = self.methods.iter().find(|existing| {
            existing.id() == method.id()
                && existing.role() == method.role()
                && existing.arity() == method.arity()
        }) {
            return Err(Error::Eval(format!(
                "generic function {} already has method {} for role {:?}",
                self.name,
                existing.id(),
                existing.role()
            )));
        }
        self.methods.push(method);
        Ok(())
    }

    /// Returns the specificity of the most specific applicable primary method.
    ///
    /// Errors if no primary method applies, or if the two most specific tie
    /// (an ambiguous primary).
    pub fn select_primary(&self, cx: &mut Cx, args: &[Value]) -> Result<MethodSpecificity> {
        let selected = self.selected_primary(cx, args)?;
        Ok(selected.specificity)
    }

    /// Returns the method ids in the order [`call`](GenericFunction::call) runs them.
    ///
    /// The plan is around (most specific first), before (most specific first),
    /// the selected primary, then after (least specific first).
    pub fn dispatch_order(&self, cx: &mut Cx, args: &[Value]) -> Result<Vec<Symbol>> {
        Ok(self
            .execution_plan(cx, args)?
            .into_iter()
            .map(|index| self.methods[index].id().clone())
            .collect())
    }

    /// Returns every applicable method's specificity, ordered for display.
    ///
    /// Sorts by combination role, then most specific first, then method id;
    /// backs the dispatch `inspect` operation.
    pub fn inspect_specificity(
        &self,
        cx: &mut Cx,
        args: &[Value],
    ) -> Result<Vec<MethodSpecificity>> {
        let mut accepted = Vec::new();
        for method in &self.methods {
            if let Some(specificity) = method.match_args(cx, args)? {
                accepted.push(specificity);
            }
        }
        accepted.sort_by(|left, right| {
            left.role()
                .combination_rank()
                .cmp(&right.role().combination_rank())
                .then_with(|| compare_specificity(right, left))
                .then_with(|| left.method().cmp(right.method()))
        });
        Ok(accepted)
    }

    /// Dispatches the call: runs the combination and returns the primary result.
    ///
    /// Executes around, before, the selected primary, and after methods in
    /// order; errors if no applicable primary method exists.
    pub fn call(&self, cx: &mut Cx, args: &[Value]) -> Result<Value> {
        let plan = self.execution_plan(cx, args)?;
        let mut primary_result = None;
        for index in plan {
            let method = &self.methods[index];
            let result = method.invoke(cx, args)?;
            if method.role() == MethodRole::Primary {
                primary_result = Some(result);
            }
        }
        primary_result.ok_or_else(|| {
            Error::Eval(format!(
                "generic function {} has no applicable primary method",
                self.name
            ))
        })
    }

    /// Dispatches the call for a named language profile.
    ///
    /// One generic function serves every profile: dispatch is profile-neutral,
    /// so this delegates to [`call`](GenericFunction::call). The `profile`
    /// argument lets profile-specific surfaces share a single generic.
    pub fn call_for_profile(
        &self,
        cx: &mut Cx,
        _profile: &Symbol,
        args: &[Value],
    ) -> Result<Value> {
        self.call(cx, args)
    }

    fn execution_plan(&self, cx: &mut Cx, args: &[Value]) -> Result<Vec<usize>> {
        let mut around = self.applicable_for_role(cx, args, MethodRole::Around)?;
        let mut before = self.applicable_for_role(cx, args, MethodRole::Before)?;
        let mut after = self.applicable_for_role(cx, args, MethodRole::After)?;
        sort_most_specific_first(&mut around);
        sort_most_specific_first(&mut before);
        sort_least_specific_first(&mut after);
        let primary = self.selected_primary(cx, args)?;

        let mut plan = Vec::with_capacity(around.len() + before.len() + 1 + after.len());
        plan.extend(around.into_iter().map(|method| method.index));
        plan.extend(before.into_iter().map(|method| method.index));
        plan.push(primary.index);
        plan.extend(after.into_iter().map(|method| method.index));
        Ok(plan)
    }

    fn selected_primary(&self, cx: &mut Cx, args: &[Value]) -> Result<ApplicableMethod> {
        let mut primary = self.applicable_for_role(cx, args, MethodRole::Primary)?;
        if primary.is_empty() {
            cx.push_diagnostic(self.selection_diagnostic("no applicable primary method"));
            return Err(Error::Eval(format!(
                "generic function {} has no applicable primary method",
                self.name
            )));
        }
        sort_most_specific_first(&mut primary);
        if primary.len() > 1
            && compare_specificity(&primary[0].specificity, &primary[1].specificity)
                == Ordering::Equal
        {
            cx.push_diagnostic(self.selection_diagnostic("ambiguous primary methods"));
            return Err(ambiguous_primary_error(
                &self.name,
                primary[0].specificity.method(),
                primary[1].specificity.method(),
            ));
        }
        Ok(primary.remove(0))
    }

    fn applicable_for_role(
        &self,
        cx: &mut Cx,
        args: &[Value],
        role: MethodRole,
    ) -> Result<Vec<ApplicableMethod>> {
        let mut accepted = Vec::new();
        for (index, method) in self.methods.iter().enumerate() {
            if method.role() != role {
                continue;
            }
            if let Some(specificity) = method.match_args(cx, args)? {
                accepted.push(ApplicableMethod { index, specificity });
            }
        }
        Ok(accepted)
    }

    fn selection_diagnostic(&self, reason: &'static str) -> Diagnostic {
        let message = format!("generic function {}: {reason}", self.name);
        let mut diagnostic = HintMetadata::new(
            Symbol::qualified("runtime-hint", "overload-selection"),
            "dispatch selection",
        )
        .with_detail(message.clone())
        .with_tag(Symbol::qualified("runtime", "dispatch"))
        .with_argument(self.name.clone())
        .attach_to(Diagnostic::error(message).with_code(Symbol::qualified("runtime", "dispatch")));
        for hint in self.operation_hints() {
            diagnostic = hint.attach_to(diagnostic);
        }
        diagnostic
    }
}

struct ApplicableMethod {
    index: usize,
    specificity: MethodSpecificity,
}

fn sort_most_specific_first(methods: &mut [ApplicableMethod]) {
    methods.sort_by(|left, right| {
        compare_specificity(&right.specificity, &left.specificity)
            .then_with(|| left.specificity.method().cmp(right.specificity.method()))
    });
}

fn sort_least_specific_first(methods: &mut [ApplicableMethod]) {
    methods.sort_by(|left, right| {
        compare_specificity(&left.specificity, &right.specificity)
            .then_with(|| left.specificity.method().cmp(right.specificity.method()))
    });
}
