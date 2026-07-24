//! Stable value fingerprints used for cutoff.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

/// A compact fingerprint for a memoized query value.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ValueFingerprint(u64);

impl ValueFingerprint {
    /// Creates a fingerprint from an already-computed stable integer.
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

/// Computes the fingerprint an incremental memo uses for cutoff.
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
