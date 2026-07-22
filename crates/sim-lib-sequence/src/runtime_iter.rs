use std::sync::Arc;

use sim_kernel::{Cx, Error, Result, Value};

use crate::{force_sequence_bounded, lazy_sequence_value};

/// Source of values addressed by contiguous integer keys.
///
/// This keeps array projection independent from any one table implementation:
/// mutation tables, language objects, or host-backed stores can all provide the
/// same integer-key lookup surface without adding a dependency edge.
pub trait RuntimeIndexSource: Send + Sync {
    /// Returns the value at `index`, or `None` to end the contiguous projection.
    fn value_at_runtime_index(&self, cx: &mut Cx, index: i64) -> Result<Option<Value>>;
}

/// Function-backed [`RuntimeIndexSource`].
pub struct RuntimeIndexLookup<F> {
    lookup: F,
}

impl<F> RuntimeIndexLookup<F> {
    /// Builds an index source from a lookup function.
    pub fn new(lookup: F) -> Self {
        Self { lookup }
    }
}

impl<F> RuntimeIndexSource for RuntimeIndexLookup<F>
where
    F: Fn(&mut Cx, i64) -> Result<Option<Value>> + Send + Sync,
{
    fn value_at_runtime_index(&self, cx: &mut Cx, index: i64) -> Result<Option<Value>> {
        (self.lookup)(cx, index)
    }
}

/// Builds a lazy sequence over contiguous integer keys starting at `first_index`.
///
/// The sequence stops at the first missing key. This is intentionally a generic
/// array projection, not a language-specific length or border rule.
pub fn runtime_index_sequence<S>(cx: &mut Cx, source: Arc<S>, first_index: i64) -> Result<Value>
where
    S: RuntimeIndexSource + 'static,
{
    lazy_sequence_value(
        cx,
        Arc::new(move |cx, offset| {
            let offset = i64::try_from(offset)
                .map_err(|_| Error::Eval("runtime index offset overflow".to_owned()))?;
            let index = first_index
                .checked_add(offset)
                .ok_or_else(|| Error::Eval("runtime index overflow".to_owned()))?;
            source.value_at_runtime_index(cx, index)
        }),
    )
}

/// Builds a lazy sequence over contiguous integer keys from a lookup function.
pub fn runtime_index_lookup_sequence<F>(cx: &mut Cx, lookup: F, first_index: i64) -> Result<Value>
where
    F: Fn(&mut Cx, i64) -> Result<Option<Value>> + Send + Sync + 'static,
{
    runtime_index_sequence(cx, Arc::new(RuntimeIndexLookup::new(lookup)), first_index)
}

/// Forces a bounded contiguous integer-key projection into a vector.
pub fn runtime_index_values<S>(
    cx: &mut Cx,
    source: Arc<S>,
    first_index: i64,
    max: usize,
    context: &str,
) -> Result<Vec<Value>>
where
    S: RuntimeIndexSource + 'static,
{
    let sequence = runtime_index_sequence(cx, source, first_index)?;
    force_sequence_bounded(cx, &sequence, max, context)
}
