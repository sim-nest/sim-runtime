//! Organ claims for the pattern surface.
//!
//! These helpers name the pattern organ and its operation keys, then publish
//! them as kernel claims so the organ and its operations are discoverable
//! through the standard Card surface.

use sim_kernel::{
    Cx, LibId, OpKey, Result, Symbol,
    standard::{publish_organ_claims, publish_organ_claims_for_lib},
};

/// Returns the organ symbol that identifies the pattern surface.
pub fn pattern_organ_symbol() -> Symbol {
    Symbol::qualified("organ", "pattern")
}

/// Returns the operation key for declaring an ADT.
pub fn pattern_adt_op_key() -> OpKey {
    pattern_op_key("adt")
}

/// Returns the operation key for constructing a tagged value.
pub fn pattern_tag_op_key() -> OpKey {
    pattern_op_key("tag")
}

/// Returns the operation key for matching a value against pattern arms.
pub fn pattern_match_op_key() -> OpKey {
    pattern_op_key("match")
}

/// Returns the operation key for destructuring a value or expression.
pub fn pattern_destructure_op_key() -> OpKey {
    pattern_op_key("destructure")
}

/// Returns the operation key for exhaustiveness checking.
pub fn pattern_exhaustive_op_key() -> OpKey {
    pattern_op_key("exhaustive")
}

/// Returns every operation key the pattern organ publishes.
pub fn pattern_op_keys() -> Vec<OpKey> {
    [
        pattern_adt_op_key(),
        pattern_tag_op_key(),
        pattern_match_op_key(),
        pattern_destructure_op_key(),
        pattern_exhaustive_op_key(),
    ]
    .into()
}

/// Publishes the pattern organ and its operation keys as kernel claims.
pub fn publish_pattern_organ_claims(cx: &mut Cx) -> Result<()> {
    publish_organ_claims(cx, pattern_organ_symbol(), pattern_op_keys())
}

/// Publishes pattern organ claims as part of a loaded lib receipt.
pub fn publish_pattern_organ_claims_for_lib(cx: &mut Cx, lib_id: LibId) -> Result<()> {
    publish_organ_claims_for_lib(cx, lib_id, pattern_organ_symbol(), pattern_op_keys())
}

fn pattern_op_key(name: &str) -> OpKey {
    OpKey::new(Symbol::new("pattern"), Symbol::new(name), 1)
}
