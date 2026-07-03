use std::sync::Arc;

use sim_kernel::{
    CORE_SEQUENCE_CLASS_ID, ClassRef, Cx, Error, Object, ObjectCompat, Result, Sequence,
    SequenceItem, Symbol, Value, seq_next_value,
};

type ValueMapper = Arc<dyn Fn(&mut Cx, Value) -> Result<Value> + Send + Sync + 'static>;
type ValuePredicate = Arc<dyn Fn(&mut Cx, &Value) -> Result<bool> + Send + Sync + 'static>;
type ValueReducer = Arc<dyn Fn(&mut Cx, Value, Value) -> Result<Value> + Send + Sync + 'static>;
type ValueVisitor = Arc<dyn Fn(&mut Cx, Value) -> Result<()> + Send + Sync + 'static>;

/// A single stage of a [`TransducerPipeline`].
#[derive(Clone)]
pub enum TransducerStep {
    /// Transform each element with the given mapper.
    Map(ValueMapper),
    /// Keep only elements satisfying the given predicate.
    Filter(ValuePredicate),
}

/// An ordered chain of [`TransducerStep`]s applied per element in one pass.
///
/// The composable core of the sequence organ's map/filter/reduce surface:
/// steps are fused so a source is traversed once, returning `None` for dropped
/// elements.
#[derive(Clone, Default)]
pub struct TransducerPipeline {
    steps: Vec<TransducerStep>,
}

impl TransducerPipeline {
    /// Start an empty pipeline.
    pub fn new() -> Self {
        Self::default()
    }

    /// Append a mapping step, consuming and returning the pipeline.
    pub fn map(mut self, mapper: ValueMapper) -> Self {
        self.steps.push(TransducerStep::Map(mapper));
        self
    }

    /// Append a filtering step, consuming and returning the pipeline.
    pub fn filter(mut self, predicate: ValuePredicate) -> Self {
        self.steps.push(TransducerStep::Filter(predicate));
        self
    }

    /// Run one element through the pipeline.
    ///
    /// Returns `Some(value)` if the element survives every step, or `None` if a
    /// filter step drops it.
    pub fn apply(&self, cx: &mut Cx, mut value: Value) -> Result<Option<Value>> {
        for step in &self.steps {
            match step {
                TransducerStep::Map(mapper) => value = mapper(cx, value)?,
                TransducerStep::Filter(predicate) => {
                    if !predicate(cx, &value)? {
                        return Ok(None);
                    }
                }
            }
        }
        Ok(Some(value))
    }
}

#[sim_citizen_derive::non_citizen(
    reason = "transduced sequence adapter; reconstruct from the source sequence and transducer pipeline descriptor",
    kind = "handle",
    descriptor = "core/Sequence"
)]
pub struct TransducedSequence {
    source: Value,
    pipeline: TransducerPipeline,
}

impl TransducedSequence {
    pub fn new(source: Value, pipeline: TransducerPipeline) -> Self {
        Self { source, pipeline }
    }
}

impl Object for TransducedSequence {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<transduced-sequence>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for TransducedSequence {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            CORE_SEQUENCE_CLASS_ID,
            Symbol::qualified("core", "Sequence"),
        )
    }

    fn as_sequence(&self) -> Option<&dyn Sequence> {
        Some(self)
    }
}

impl Sequence for TransducedSequence {
    fn next_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        while let Some(item) = seq_next_value(cx, &self.source)? {
            let value = item.into_value(cx)?;
            if let Some(value) = self.pipeline.apply(cx, value)? {
                return Ok(Some(SequenceItem::new(value)));
            }
        }
        Ok(None)
    }

    fn close(&self, cx: &mut Cx) -> Result<()> {
        if let Some(sequence) = self.source.object().as_sequence() {
            sequence.close(cx)
        } else {
            Ok(())
        }
    }
}

/// Return a lazy sequence that maps each element of `source`.
///
/// Fails if `source` is not a sequence.
pub fn map_sequence(cx: &mut Cx, source: Value, mapper: ValueMapper) -> Result<Value> {
    transduced_sequence_value(cx, source, TransducerPipeline::new().map(mapper))
}

/// Return a lazy sequence that keeps elements of `source` passing `predicate`.
///
/// Fails if `source` is not a sequence.
pub fn filter_sequence(cx: &mut Cx, source: Value, predicate: ValuePredicate) -> Result<Value> {
    transduced_sequence_value(cx, source, TransducerPipeline::new().filter(predicate))
}

/// Fold `source` into a single value with `reducer`, starting from `init`.
pub fn reduce_sequence(
    cx: &mut Cx,
    source: &Value,
    init: Value,
    reducer: ValueReducer,
) -> Result<Value> {
    transduce(cx, source, TransducerPipeline::new(), init, reducer)
}

/// Apply `visitor` to each element of `source` for its effect.
pub fn for_each_sequence(cx: &mut Cx, source: &Value, visitor: ValueVisitor) -> Result<()> {
    while let Some(item) = seq_next_value(cx, source)? {
        let value = item.into_value(cx)?;
        visitor(cx, value)?;
    }
    Ok(())
}

/// Stream `source` through `pipeline` and fold survivors into a single value.
///
/// The one-pass core combining transformation and reduction: each element is
/// run through `pipeline`, and surviving elements are folded with `reducer`
/// from `init`.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, Expr, NoopEvalPolicy, NumberLiteral, Symbol, Value};
/// use sim_lib_sequence::{lazy_sequence_value, transduce, TransducerPipeline};
///
/// fn n(cx: &mut Cx, v: u64) -> Value {
///     cx.factory().number_literal(Symbol::qualified("test", "u64"), v.to_string()).unwrap()
/// }
/// fn read(cx: &mut Cx, v: &Value) -> u64 {
///     let Expr::Number(NumberLiteral { canonical, .. }) = v.object().as_expr(cx).unwrap() else {
///         panic!("number");
///     };
///     canonical.parse().unwrap()
/// }
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// // Source yields 0, 1, 2 then ends.
/// let source = lazy_sequence_value(&mut cx, Arc::new(|cx: &mut Cx, i| {
///     if i >= 3 { return Ok(None); }
///     Ok(Some(n(cx, i as u64)))
/// }))?;
///
/// let zero = n(&mut cx, 0);
/// let sum = transduce(
///     &mut cx,
///     &source,
///     TransducerPipeline::new(),
///     zero,
///     Arc::new(|cx, acc, value| {
///         let total = read(cx, &acc) + read(cx, &value);
///         Ok(n(cx, total))
///     }),
/// )?;
/// assert_eq!(read(&mut cx, &sum), 3);
/// # Ok::<(), sim_kernel::Error>(())
/// ```
pub fn transduce(
    cx: &mut Cx,
    source: &Value,
    pipeline: TransducerPipeline,
    mut init: Value,
    reducer: ValueReducer,
) -> Result<Value> {
    while let Some(item) = seq_next_value(cx, source)? {
        let value = item.into_value(cx)?;
        if let Some(value) = pipeline.apply(cx, value)? {
            init = reducer(cx, init, value)?;
        }
    }
    Ok(init)
}

fn transduced_sequence_value(
    cx: &mut Cx,
    source: Value,
    pipeline: TransducerPipeline,
) -> Result<Value> {
    if source.object().as_sequence().is_none() {
        return Err(Error::TypeMismatch {
            expected: "sequence",
            found: "non-sequence",
        });
    }
    cx.factory()
        .opaque(Arc::new(TransducedSequence::new(source, pipeline)))
}
