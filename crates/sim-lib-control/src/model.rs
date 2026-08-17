use sim_kernel::{
    ClassRef, Cx, Error, Expr, MatchScore, Object, ObjectCompat, ObjectEncode, ObjectEncoding,
    Origin, Ref, Result, Shape, ShapeDoc, ShapeMatch, Symbol, Value,
};

/// Stable read-construct identity for a raised completion envelope.
pub const RAISED_SYMBOL: &str = "control/Raised";

fn raised_symbol() -> Symbol {
    Symbol::qualified("control", "Raised")
}

/// Explicit byte budget for rendering a raised payload into a browse face.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RaisedBrowseBudget {
    max_payload_bytes: usize,
}

impl RaisedBrowseBudget {
    /// Creates a non-zero payload rendering budget.
    pub fn new(max_payload_bytes: usize) -> Result<Self> {
        if max_payload_bytes == 0 {
            return Err(Error::Eval(
                "raised browse payload budget must be non-zero".to_owned(),
            ));
        }
        Ok(Self { max_payload_bytes })
    }

    /// Returns the maximum rendered payload bytes.
    pub fn max_payload_bytes(self) -> usize {
        self.max_payload_bytes
    }
}

/// Result of a bounded raised-envelope browse projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RaisedBrowseProjection {
    /// UTF-8 payload prefix fitting the requested budget.
    pub payload: String,
    /// Whether bytes were omitted from the payload display.
    pub truncated: bool,
    /// Full payload display size before truncation.
    pub original_payload_bytes: usize,
}

/// The one language-neutral exceptional-completion envelope.
///
/// Recursive relations such as causes, contexts, groups, and suppressed
/// exceptions belong to guest-owned managed payload objects. They are never
/// fields of this envelope.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Raised {
    class: ClassRef,
    payload: Value,
    origin: Origin,
    profile: Symbol,
}

impl Raised {
    /// Builds a checked, non-recursive raised envelope.
    ///
    /// A raised envelope cannot itself be used as the immediate payload. Guest
    /// relation graphs must instead live in their ordinary managed objects.
    pub fn new(class: ClassRef, payload: Value, origin: Origin, profile: Symbol) -> Result<Self> {
        if payload.object().downcast_ref::<Self>().is_some() {
            return Err(Error::Eval(
                "Raised payload cannot be another Raised envelope".to_owned(),
            ));
        }
        Ok(Self {
            class,
            payload,
            origin,
            profile,
        })
    }

    /// Returns the kernel class identity used for handler matching.
    pub fn class_ref(&self) -> &ClassRef {
        &self.class
    }

    /// Returns the ordinary guest payload value.
    pub fn payload(&self) -> &Value {
        &self.payload
    }

    /// Returns deterministic source provenance for the raise site.
    pub fn origin(&self) -> &Origin {
        &self.origin
    }

    /// Returns the stable guest-profile identity used for policy routing.
    pub fn profile(&self) -> &Symbol {
        &self.profile
    }

    /// Renders the payload under an explicit byte budget and reports loss.
    pub fn browse(
        &self,
        cx: &mut Cx,
        budget: RaisedBrowseBudget,
    ) -> Result<RaisedBrowseProjection> {
        let rendered = self.payload.object().display(cx)?;
        let original_payload_bytes = rendered.len();
        let mut end = rendered.len().min(budget.max_payload_bytes);
        while !rendered.is_char_boundary(end) {
            end -= 1;
        }
        Ok(RaisedBrowseProjection {
            payload: rendered[..end].to_owned(),
            truncated: end < rendered.len(),
            original_payload_bytes,
        })
    }

    fn constructor_args(&self, cx: &mut Cx) -> Result<Vec<Expr>> {
        Ok(vec![
            self.class.object().as_expr(cx)?,
            self.payload.object().as_expr(cx)?,
            Expr::Vector(vec![
                Expr::String(self.origin.codec.0.to_string()),
                Expr::String(self.origin.source.0.clone()),
                Expr::String(self.origin.span.start.to_string()),
                Expr::String(self.origin.span.end.to_string()),
                Expr::String(format!("{:?}", self.origin.trivia)),
            ]),
            Expr::Symbol(self.profile.clone()),
        ])
    }
}

impl Object for Raised {
    fn display(&self, cx: &mut Cx) -> Result<String> {
        let projection = self.browse(cx, RaisedBrowseBudget::new(256)?)?;
        let suffix = if projection.truncated {
            " [truncated]"
        } else {
            ""
        };
        Ok(format!(
            "#<{} profile={} payload={}{}>",
            RAISED_SYMBOL, self.profile, projection.payload, suffix
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for Raised {
    fn as_expr(&self, cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Extension {
            tag: Symbol::qualified("citizen", "read-construct"),
            payload: Box::new(Expr::Vector(vec![
                Expr::Symbol(raised_symbol()),
                Expr::Vector(self.constructor_args(cx)?),
            ])),
        })
    }

    fn as_table(&self, cx: &mut Cx) -> Result<Value> {
        let projection = self.browse(cx, RaisedBrowseBudget::new(256)?)?;
        cx.factory().table(vec![
            (Symbol::new("class"), self.class.clone()),
            (Symbol::new("payload"), self.payload.clone()),
            (
                Symbol::new("origin-source"),
                cx.factory().string(self.origin.source.0.clone())?,
            ),
            (
                Symbol::new("profile"),
                cx.factory().symbol(self.profile.clone())?,
            ),
            (
                Symbol::new("payload-rendered"),
                cx.factory().string(projection.payload)?,
            ),
            (
                Symbol::new("payload-truncated"),
                cx.factory().bool(projection.truncated)?,
            ),
        ])
    }

    fn as_object_encoder(&self) -> Option<&dyn ObjectEncode> {
        Some(self)
    }
}

impl ObjectEncode for Raised {
    fn object_encoding(&self, cx: &mut Cx) -> Result<ObjectEncoding> {
        Ok(ObjectEncoding::Constructor {
            class: raised_symbol(),
            args: self.constructor_args(cx)?,
        })
    }
}

/// Shape for the non-recursive [`Raised`] object and its read-construct face.
#[derive(Clone, Copy, Debug, Default)]
pub struct RaisedShape;

impl Shape for RaisedShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(raised_symbol())
    }

    fn check_value(&self, _cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        Ok(if value.object().downcast_ref::<Raised>().is_some() {
            ShapeMatch::accept(MatchScore::exact(1))
        } else {
            ShapeMatch::reject("expected control/Raised")
        })
    }

    fn check_expr(&self, _cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let accepted = matches!(expr, Expr::Extension { tag, payload }
            if tag == &Symbol::qualified("citizen", "read-construct")
                && matches!(payload.as_ref(), Expr::Vector(parts)
                    if matches!(parts.as_slice(), [Expr::Symbol(class), Expr::Vector(args)]
                        if class == &raised_symbol() && args.len() == 4)));
        Ok(if accepted {
            ShapeMatch::accept(MatchScore::exact(1))
        } else {
            ShapeMatch::reject("expected four-field control/Raised read-construct")
        })
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("non-recursive raised completion envelope")
            .with_detail("class, payload, origin, and profile"))
    }
}

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
    kind = "marker",
    descriptor = "core/Ref"
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

#[cfg(test)]
mod raised_tests {
    use std::sync::{Arc, Mutex};

    use sim_kernel::{CodecId, DefaultFactory, NoopEvalPolicy, Span};

    use crate::{
        CleanupStack, FrameLimits, RaisedResumePacket, RaisedResumeResult, RaisedUnwind,
        ResumableFrame,
    };

    use super::*;

    fn fixture(cx: &mut Cx, payload: &str) -> Raised {
        Raised::new(
            cx.factory().symbol(Symbol::new("guest/Error")).unwrap(),
            cx.factory().string(payload.to_owned()).unwrap(),
            Origin {
                codec: CodecId(7),
                source: sim_kernel::SourceId("fixture.sim".to_owned()),
                span: Span { start: 2, end: 9 },
                trivia: Vec::new(),
            },
            Symbol::new("guest/profile-v1"),
        )
        .unwrap()
    }

    #[test]
    fn checked_constructor_rejects_a_raised_payload() {
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let inner = fixture(&mut cx, "inner");
        let inner = cx.factory().opaque(Arc::new(inner)).unwrap();
        let error = Raised::new(
            cx.factory().symbol(Symbol::new("guest/Error")).unwrap(),
            inner,
            Origin {
                codec: CodecId(7),
                source: sim_kernel::SourceId("fixture.sim".to_owned()),
                span: Span { start: 0, end: 1 },
                trivia: Vec::new(),
            },
            Symbol::new("guest/profile-v1"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("cannot be another Raised"));
    }

    #[test]
    fn api_shape_has_exactly_four_non_recursive_fields() {
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let Raised {
            class,
            payload,
            origin,
            profile,
        } = fixture(&mut cx, "payload");
        let _: ClassRef = class;
        let _: Value = payload;
        let _: Origin = origin;
        let _: Symbol = profile;
    }

    #[test]
    fn browse_and_shape_report_budget_truncation_and_read_construct() {
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let raised = fixture(&mut cx, "abcdefgh");
        assert_eq!(
            raised
                .browse(&mut cx, RaisedBrowseBudget::new(4).unwrap())
                .unwrap(),
            RaisedBrowseProjection {
                payload: "abcd".to_owned(),
                truncated: true,
                original_payload_bytes: 8,
            }
        );
        let expr = raised.as_expr(&mut cx).unwrap();
        assert!(RaisedShape.check_expr(&mut cx, &expr).unwrap().accepted);
        let value = cx.factory().opaque(Arc::new(raised)).unwrap();
        assert!(RaisedShape.check_value(&mut cx, value).unwrap().accepted);
    }

    #[test]
    fn raised_unwinds_two_cleanups_then_resumes_with_stable_receipts() {
        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let receipts = Arc::new(Mutex::new(Vec::new()));
        let mut cleanups: CleanupStack<RaisedUnwind<(), (), ()>> = CleanupStack::new();
        for name in ["outer", "inner"] {
            let receipts = Arc::clone(&receipts);
            cleanups.push(move |_| receipts.lock().unwrap().push(name));
        }
        let reason = cleanups.unwind(RaisedUnwind::Exception(fixture(&mut cx, "boom")));
        let RaisedUnwind::Exception(raised) = reason else {
            unreachable!()
        };
        // Start establishes the frame; the characterized resume follows a
        // protected suspension, so model that suspension with Yielded first.
        let mut frame = ResumableFrame::new(FrameLimits { depth: 2, work: 2 }, {
            let mut started = false;
            move |packet: RaisedResumePacket<()>, _: &mut crate::StepBudget| match packet {
                RaisedResumePacket::Start if !started => {
                    started = true;
                    Ok::<_, crate::FrameError>(RaisedResumeResult::Yielded(()))
                }
                RaisedResumePacket::Throw(raised) => Ok(RaisedResumeResult::Failed(raised)),
                _ => unreachable!(),
            }
        });
        assert!(matches!(
            frame.resume::<(), (), Raised>(RaisedResumePacket::Start),
            Ok(RaisedResumeResult::Yielded(()))
        ));
        assert!(matches!(
            frame.resume::<(), (), Raised>(RaisedResumePacket::Throw(raised)),
            Ok(RaisedResumeResult::Failed(_))
        ));
        receipts.lock().unwrap().push("resumed");
        assert_eq!(&*receipts.lock().unwrap(), &["inner", "outer", "resumed"]);
    }
}
