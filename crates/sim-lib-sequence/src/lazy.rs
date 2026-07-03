use std::sync::{Arc, Mutex};

use sim_kernel::{
    CORE_SEQUENCE_CLASS_ID, ClassRef, Cx, Error, ListSequence, Object, ObjectCompat, Result,
    Sequence, SequenceItem, Symbol, Value, seq_next_value,
};

/// Element generator backing a [`LazySequence`].
///
/// Called with the zero-based index of the next element; returns `Some(value)`
/// to yield it or `None` to signal exhaustion.
pub type SequenceProducer =
    Arc<dyn Fn(&mut Cx, usize) -> Result<Option<Value>> + Send + Sync + 'static>;

#[sim_citizen_derive::non_citizen(
    reason = "live lazy sequence producer; reconstruct from the source sequence expression",
    kind = "handle",
    descriptor = "core/Sequence"
)]
/// On-demand sequence object driven by a [`SequenceProducer`].
///
/// Implements the kernel [`Sequence`] contract over a closure: elements are
/// pulled lazily and cached for peeking, so the kernel sequence protocol drives
/// arbitrary generated data without materializing it.
#[derive(Clone)]
pub struct LazySequence {
    producer: SequenceProducer,
    state: Arc<Mutex<LazySequenceState>>,
}

#[derive(Debug, Default)]
struct LazySequenceState {
    index: usize,
    pending: Option<SequenceItem>,
    done: bool,
    closed: bool,
}

impl LazySequence {
    /// Build a lazy sequence from an element producer.
    pub fn new(producer: SequenceProducer) -> Self {
        Self {
            producer,
            state: Arc::new(Mutex::new(LazySequenceState::default())),
        }
    }

    fn produce(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        let index = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::PoisonedLock("lazy sequence"))?;
            if state.closed || state.done {
                return Ok(None);
            }
            let index = state.index;
            state.index += 1;
            index
        };
        match (self.producer)(cx, index)? {
            Some(value) => Ok(Some(SequenceItem::new(value))),
            None => {
                self.mark_done()?;
                Ok(None)
            }
        }
    }

    fn mark_done(&self) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("lazy sequence"))?;
        state.done = true;
        Ok(())
    }
}

impl Object for LazySequence {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<lazy-sequence>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LazySequence {
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

impl Sequence for LazySequence {
    fn next_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::PoisonedLock("lazy sequence"))?;
            if state.closed {
                return Ok(None);
            }
            if let Some(item) = state.pending.take() {
                return Ok(Some(item));
            }
        }
        self.produce(cx)
    }

    fn close(&self, _cx: &mut Cx) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("lazy sequence"))?;
        state.pending = None;
        state.closed = true;
        state.done = true;
        Ok(())
    }

    fn peek_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        {
            let state = self
                .state
                .lock()
                .map_err(|_| Error::PoisonedLock("lazy sequence"))?;
            if state.closed {
                return Ok(None);
            }
            if let Some(item) = state.pending.clone() {
                return Ok(Some(item));
            }
        }
        let item = self.produce(cx)?;
        if let Some(item) = item.clone() {
            let mut state = self
                .state
                .lock()
                .map_err(|_| Error::PoisonedLock("lazy sequence"))?;
            state.pending = Some(item);
        }
        Ok(item)
    }

    fn is_done(&self, _cx: &mut Cx) -> Result<bool> {
        let state = self
            .state
            .lock()
            .map_err(|_| Error::PoisonedLock("lazy sequence"))?;
        Ok((state.done || state.closed) && state.pending.is_none())
    }
}

/// Wrap a [`SequenceProducer`] as a runtime sequence [`Value`].
///
/// The primary entry point for constructing lazy sequences: builds a
/// [`LazySequence`] and boxes it as an opaque kernel object.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Symbol};
/// use sim_lib_sequence::{force_sequence_bounded, lazy_sequence_value};
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// // A producer of two elements, then exhaustion.
/// let seq = lazy_sequence_value(&mut cx, Arc::new(|cx: &mut Cx, index| {
///     if index >= 2 {
///         return Ok(None);
///     }
///     let value = cx
///         .factory()
///         .number_literal(Symbol::qualified("test", "u64"), index.to_string())?;
///     Ok(Some(value))
/// }))?;
///
/// let forced = force_sequence_bounded(&mut cx, &seq, 8, "doc")?;
/// assert_eq!(forced.len(), 2);
/// # Ok::<(), sim_kernel::Error>(())
/// ```
pub fn lazy_sequence_value(cx: &mut Cx, producer: SequenceProducer) -> Result<Value> {
    cx.factory().opaque(Arc::new(LazySequence::new(producer)))
}

/// Adapt a list [`Value`] into a sequence [`Value`].
///
/// Bridges the kernel list contract to the sequence contract so list data can
/// be consumed through the sequence organ.
pub fn sequence_from_list_value(cx: &mut Cx, list: Value) -> Result<Value> {
    cx.factory().opaque(Arc::new(ListSequence::new(list)))
}

/// Drive a sequence to completion, refusing to exceed `max` elements.
///
/// Pulls up to `max` elements; if the sequence still has more, fails with
/// `context` in the message. Guards the sequence organ against unbounded
/// lazy producers.
pub fn force_sequence_bounded(
    cx: &mut Cx,
    sequence: &Value,
    max: usize,
    context: &str,
) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for _ in 0..max {
        let Some(item) = seq_next_value(cx, sequence)? else {
            return Ok(out);
        };
        out.push(item.into_value(cx)?);
    }
    if sequence_has_more(cx, sequence)? {
        return Err(Error::Eval(format!(
            "{context}: sequence exceeds force bound {max}"
        )));
    }
    Ok(out)
}

fn sequence_has_more(cx: &mut Cx, sequence: &Value) -> Result<bool> {
    if let Some(sequence) = sequence.object().as_sequence() {
        return sequence.peek_item(cx).map(|item| item.is_some());
    }
    seq_next_value(cx, sequence).map(|item| item.is_some())
}
