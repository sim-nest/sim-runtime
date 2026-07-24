//! Organ identity and operation claims for the incremental query organ.

use sim_kernel::{
    Cx, LibId, OpKey, Result, Symbol,
    standard::{publish_organ_claims, publish_organ_claims_for_lib},
};

/// The symbol that identifies the incremental query organ in the claim store.
pub fn incremental_organ_symbol() -> Symbol {
    Symbol::qualified("organ", "incremental")
}

/// Operation key for creating an incremental engine session.
pub fn incremental_engine_op_key() -> OpKey {
    incremental_op_key("engine")
}

/// Operation key for registering a query expression.
pub fn incremental_register_op_key() -> OpKey {
    incremental_op_key("register")
}

/// Operation key for invalidating a query or observed external key.
pub fn incremental_invalidate_op_key() -> OpKey {
    incremental_op_key("invalidate")
}

/// Operation key for verifying a root query.
pub fn incremental_verify_op_key() -> OpKey {
    incremental_op_key("verify")
}

/// Operation key for explaining a registered query memo.
pub fn incremental_explain_op_key() -> OpKey {
    incremental_op_key("explain")
}

/// Operation key for exporting a reachable memo snapshot.
pub fn incremental_snapshot_op_key() -> OpKey {
    incremental_op_key("snapshot")
}

/// Operation key for reporting engine metrics.
pub fn incremental_metrics_op_key() -> OpKey {
    incremental_op_key("metrics")
}

/// The full set of operation keys exposed by the incremental organ.
pub fn incremental_op_keys() -> Vec<OpKey> {
    [
        incremental_engine_op_key(),
        incremental_register_op_key(),
        incremental_invalidate_op_key(),
        incremental_verify_op_key(),
        incremental_explain_op_key(),
        incremental_snapshot_op_key(),
        incremental_metrics_op_key(),
    ]
    .into()
}

/// Publish the incremental query organ and its operation keys as claims.
pub fn publish_incremental_organ_claims(cx: &mut Cx) -> Result<()> {
    publish_organ_claims(cx, incremental_organ_symbol(), incremental_op_keys())
}

/// Publish the incremental query organ claims as part of a loaded lib receipt.
pub fn publish_incremental_organ_claims_for_lib(cx: &mut Cx, lib_id: LibId) -> Result<()> {
    publish_organ_claims_for_lib(
        cx,
        lib_id,
        incremental_organ_symbol(),
        incremental_op_keys(),
    )
}

fn incremental_op_key(name: &str) -> OpKey {
    OpKey::new(Symbol::new("incremental"), Symbol::new(name), 1)
}
