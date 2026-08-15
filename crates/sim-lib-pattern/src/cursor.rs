//! Type-separated subject symbols and pattern cursors.

use core::fmt::Debug;
use core::marker::PhantomData;

/// Checked exact-text offset types owned by `sim-text`.
pub use sim_text::{CodeUnitOffset, ScalarOffset};

/// A symbol domain defines both the unit consumed by a pattern and its offset type.
///
/// Keeping the offset as an associated type prevents byte-oriented patterns from
/// accidentally being paired with subjects indexed in code units.
pub trait SymbolDomain {
    /// One symbol consumed from a subject.
    type Symbol;
    /// A position in both the pattern source and the subject.
    type Offset: Copy + Debug + Eq + Ord;
}

/// A byte offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ByteOffset(pub usize);

/// A domain whose symbols and offsets are bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ByteDomain;

impl SymbolDomain for ByteDomain {
    type Symbol = u8;
    type Offset = ByteOffset;
}

/// A domain whose symbols and offsets are Unicode scalar values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScalarDomain;

impl SymbolDomain for ScalarDomain {
    type Symbol = char;
    type Offset = ScalarOffset;
}

/// A domain whose symbols and offsets are exact UTF-16 code units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CodeUnitDomain;

impl SymbolDomain for CodeUnitDomain {
    type Symbol = u16;
    type Offset = CodeUnitOffset;
}

/// A matching cursor carrying independent source and subject positions.
///
/// ```compile_fail
/// use sim_lib_pattern::{ByteDomain, ByteOffset, CodeUnitDomain, CodeUnitOffset, Cursor};
///
/// fn run_bytes(_: Cursor<ByteDomain>) {}
/// let text_cursor = Cursor::<CodeUnitDomain>::new(CodeUnitOffset::new(0), CodeUnitOffset::new(0));
/// run_bytes(text_cursor);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor<D: SymbolDomain> {
    source: D::Offset,
    subject: D::Offset,
    domain: PhantomData<fn() -> D>,
}

impl<D: SymbolDomain> Cursor<D> {
    /// Creates a cursor from its pattern-source and subject positions.
    pub const fn new(source: D::Offset, subject: D::Offset) -> Self {
        Self {
            source,
            subject,
            domain: PhantomData,
        }
    }

    /// Returns the position in the pattern source.
    pub const fn source_position(&self) -> D::Offset {
        self.source
    }

    /// Returns the position in the matched subject.
    pub const fn subject_position(&self) -> D::Offset {
        self.subject
    }

    /// Returns a cursor advanced to new source and subject positions.
    pub const fn advanced(self, source: D::Offset, subject: D::Offset) -> Self {
        Self::new(source, subject)
    }
}
