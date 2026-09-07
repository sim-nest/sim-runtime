//! Process-local value fingerprints used only for recomputation cutoff.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

/// A compact process-local hint for a memoized query value.
///
/// This value must never cross a persistence boundary or authorize an effect,
/// replay, omission, or durable reuse decision. A collision may only affect
/// cutoff behavior inside one live engine.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ValueFingerprint(u64);

impl ValueFingerprint {
    /// Creates a process-local hint from an already-computed integer.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw fingerprint bits.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Computes the process-local fingerprint an incremental memo uses for cutoff.
pub trait FingerprintValue {
    /// Returns a compact value identity for incremental cutoff.
    fn incremental_fingerprint(&self) -> ValueFingerprint;
}

impl<T> FingerprintValue for T
where
    T: Hash,
{
    fn incremental_fingerprint(&self) -> ValueFingerprint {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        ValueFingerprint(hasher.finish())
    }
}
