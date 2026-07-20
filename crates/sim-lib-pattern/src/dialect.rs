//! Dialect compiler seam for text-pattern syntaxes.

use sim_kernel::Result;

use crate::TextOp;

/// A surface pattern syntax that compiles to the shared text-pattern VM.
pub trait PatternDialect: Send + Sync {
    /// Compiles `pattern` into VM operations.
    ///
    /// # Errors
    ///
    /// Returns an error when the surface pattern is malformed.
    fn compile(&self, pattern: &str) -> Result<Vec<TextOp>>;
}
