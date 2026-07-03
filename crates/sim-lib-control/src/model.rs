use sim_kernel::{Cx, Expr, Object, ObjectCompat, Ref, Result, Symbol};

#[sim_citizen_derive::non_citizen(
    reason = "live continuation capture handle; descriptor data is the continuation and capture refs",
    kind = "handle",
    descriptor = "core/Ref"
)]
/// A runtime object wrapping a captured continuation and its capture result.
///
/// Returned when a control capture succeeds; carries the continuation [`Ref`]
/// to resume, the result the capture produced, and whether the continuation may
/// be resumed more than once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinuationValue {
    continuation: Ref,
    capture_result: Ref,
    multishot: bool,
}

impl ContinuationValue {
    /// Wraps a captured `continuation`, its `capture_result`, and whether it is
    /// `multishot` (resumable more than once).
    pub fn new(continuation: Ref, capture_result: Ref, multishot: bool) -> Self {
        Self {
            continuation,
            capture_result,
            multishot,
        }
    }

    /// Returns the continuation reference to resume.
    pub fn continuation(&self) -> &Ref {
        &self.continuation
    }

    /// Returns the result produced when the continuation was captured.
    pub fn capture_result(&self) -> &Ref {
        &self.capture_result
    }

    /// Returns whether this continuation may be resumed more than once.
    pub fn multishot(&self) -> bool {
        self.multishot
    }
}

impl Object for ContinuationValue {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<control-continuation {:?}>", self.continuation))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ContinuationValue {
    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Call {
            operator: Box::new(Expr::Symbol(Symbol::qualified("control", "continuation"))),
            args: vec![ref_expr(&self.continuation)],
        })
    }
}

#[sim_citizen_derive::non_citizen(
    reason = "control result ref wrapper; canonical data is the referenced value",
    kind = "marker"
)]
/// A runtime object wrapping the result reference of a control operation.
///
/// Produced by prompt, abort, and resume operations; its canonical data is the
/// referenced value it carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlResultValue {
    reference: Ref,
}

impl ControlResultValue {
    /// Wraps the `reference` produced by a control operation.
    pub fn new(reference: Ref) -> Self {
        Self { reference }
    }

    /// Returns the wrapped result reference.
    pub fn reference(&self) -> &Ref {
        &self.reference
    }
}

impl Object for ControlResultValue {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<control-result {:?}>", self.reference))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ControlResultValue {
    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(ref_expr(&self.reference))
    }
}

pub(crate) fn ref_expr(reference: &Ref) -> Expr {
    match reference {
        Ref::Symbol(symbol) => Expr::Symbol(symbol.clone()),
        other => Expr::String(format!("{other:?}")),
    }
}
