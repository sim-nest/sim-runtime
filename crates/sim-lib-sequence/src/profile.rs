use std::sync::Arc;

use sim_kernel::{
    CORE_SEQUENCE_CLASS_ID, ClassRef, Cx, Object, ObjectCompat, Result, Sequence, SequenceItem,
    Symbol, Value, seq_close_value, seq_is_done, seq_next_value, seq_peek,
};

#[sim_citizen_derive::non_citizen(
    reason = "profile sequence adapter; reconstruct from the profile symbol and source sequence descriptor",
    kind = "handle",
    descriptor = "core/Sequence"
)]
/// Sequence object that tags an inner sequence with a language profile.
///
/// A transparent adapter implementing the kernel [`Sequence`] contract by
/// delegating to its inner sequence, letting sequence values cross language
/// profile boundaries while carrying their originating profile.
pub struct ProfileSequence {
    profile: Symbol,
    inner: Value,
}

impl ProfileSequence {
    /// Wrap an inner sequence, tagging it with `profile`.
    pub fn new(profile: Symbol, inner: Value) -> Self {
        Self { profile, inner }
    }

    /// The language profile this sequence is tagged with.
    pub fn profile(&self) -> &Symbol {
        &self.profile
    }
}

impl Object for ProfileSequence {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<profile-sequence {}>", self.profile))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ProfileSequence {
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

impl Sequence for ProfileSequence {
    fn next_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        seq_next_value(cx, &self.inner)
    }

    fn close(&self, cx: &mut Cx) -> Result<()> {
        seq_close_value(cx, &self.inner)
    }

    fn peek_item(&self, cx: &mut Cx) -> Result<Option<SequenceItem>> {
        match self.inner.object().as_sequence() {
            Some(sequence) => seq_peek(cx, sequence),
            None => Ok(None),
        }
    }

    fn is_done(&self, cx: &mut Cx) -> Result<bool> {
        match self.inner.object().as_sequence() {
            Some(sequence) => seq_is_done(cx, sequence),
            None => Ok(false),
        }
    }
}

/// Tag a sequence [`Value`] with a language profile via [`ProfileSequence`].
pub fn sequence_for_profile(cx: &mut Cx, profile: Symbol, sequence: Value) -> Result<Value> {
    cx.factory()
        .opaque(Arc::new(ProfileSequence::new(profile, sequence)))
}
