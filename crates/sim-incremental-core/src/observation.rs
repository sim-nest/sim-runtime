//! Dependency observation records captured during query execution.

use crate::ValueFingerprint;

/// A monotone revision stamp for memoized values and external observations.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Revision(u64);

impl Revision {
    /// The initial revision assigned to unseen external observations.
    pub const ZERO: Self = Self(0);

    /// Creates a revision from raw stamp bits.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw revision bits.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The reason a query depends on a key.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ObservationKind {
    /// A query read another query's value.
    Read,
    /// A query observed that a name was absent.
    Missing,
    /// A query observed a directory or collection listing.
    Listing,
    /// A query observed policy or authority state.
    Policy,
    /// A query observed an external backend epoch.
    Epoch,
    /// A domain-specific observation class.
    Custom(&'static str),
}

/// One dependency observation captured by a query frame.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Observation<K> {
    key: K,
    kind: ObservationKind,
    revision: Revision,
    fingerprint: Option<ValueFingerprint>,
}

impl<K> Observation<K> {
    /// Creates an observation from a key, kind, revision, and optional value
    /// fingerprint.
    #[must_use]
    pub fn new(
        key: K,
        kind: ObservationKind,
        revision: Revision,
        fingerprint: Option<ValueFingerprint>,
    ) -> Self {
        Self {
            key,
            kind,
            revision,
            fingerprint,
        }
    }

    /// Creates a query-read observation.
    #[must_use]
    pub fn read(key: K, revision: Revision, fingerprint: ValueFingerprint) -> Self {
        Self::new(key, ObservationKind::Read, revision, Some(fingerprint))
    }

    /// Returns the observed key.
    #[must_use]
    pub fn key(&self) -> &K {
        &self.key
    }

    /// Returns the observation kind.
    #[must_use]
    pub fn kind(&self) -> &ObservationKind {
        &self.kind
    }

    /// Returns the revision captured by the observation.
    #[must_use]
    pub fn revision(&self) -> Revision {
        self.revision
    }

    /// Returns the captured value fingerprint when this observation has one.
    #[must_use]
    pub fn fingerprint(&self) -> Option<ValueFingerprint> {
        self.fingerprint
    }
}
