use sim_kernel::{
    Cx, LibId, OpKey, Result, Symbol,
    standard::{publish_organ_claims, publish_organ_claims_for_lib},
};

use crate::runtime::SeqOp;

/// Symbol naming the sequence organ as a claim subject.
///
/// Identifies this crate's behavior in the kernel claim store so the organ and
/// its operations project into a browse Card.
pub fn sequence_organ_symbol() -> Symbol {
    Symbol::qualified("organ", "sequence")
}

/// Operation key for persistent sequence construction.
pub fn sequence_persistent_op_key() -> OpKey {
    sequence_op_key("persistent")
}

/// Operation key for lazy sequence construction.
pub fn sequence_lazy_op_key() -> OpKey {
    sequence_op_key("lazy")
}

/// Operation key for sequence mapping.
pub fn sequence_map_op_key() -> OpKey {
    sequence_op_key("map")
}

/// Operation key for sequence filtering.
pub fn sequence_filter_op_key() -> OpKey {
    sequence_op_key("filter")
}

/// Operation key for sequence reduction.
pub fn sequence_reduce_op_key() -> OpKey {
    sequence_op_key("reduce")
}

/// Operation key for sequence iteration (`for-each`).
pub fn sequence_for_op_key() -> OpKey {
    sequence_op_key("for")
}

/// Operation key for transducer-driven sequence pipelines.
pub fn sequence_transduce_op_key() -> OpKey {
    sequence_op_key("transduce")
}

/// All sequence-surface operations this crate models, whether or not they are
/// currently exported as live runtime callables.
///
/// The canonical operation set published with
/// [`publish_sequence_organ_claims`].
pub fn sequence_declared_op_keys() -> Vec<OpKey> {
    [
        sequence_persistent_op_key(),
        sequence_lazy_op_key(),
        sequence_map_op_key(),
        sequence_filter_op_key(),
        sequence_reduce_op_key(),
        sequence_for_op_key(),
        sequence_transduce_op_key(),
    ]
    .into()
}

/// Live sequence claim-to-export mappings backed by the loaded runtime surface.
pub fn sequence_live_ops() -> Vec<(OpKey, Symbol)> {
    vec![
        (sequence_map_op_key(), SeqOp::Map.symbol()),
        (sequence_filter_op_key(), SeqOp::Filter.symbol()),
        (sequence_reduce_op_key(), SeqOp::Fold.symbol()),
    ]
}

/// Operation keys the sequence organ currently publishes as live claims.
pub fn sequence_op_keys() -> Vec<OpKey> {
    sequence_live_ops()
        .into_iter()
        .map(|(op_key, _export_symbol)| op_key)
        .collect()
}

/// Publish the sequence organ and its operation keys into the claim store.
///
/// Realizes the kernel organ-claim contract for this crate: the kernel defines
/// claim and Card contracts, this records the concrete sequence organ so it is
/// discoverable. See the [crate README](https://github.com/sim-nest/sim-runtime).
pub fn publish_sequence_organ_claims(cx: &mut Cx) -> Result<()> {
    publish_organ_claims(cx, sequence_organ_symbol(), sequence_op_keys())
}

/// Publish the sequence organ claims as part of a loaded lib receipt.
pub fn publish_sequence_organ_claims_for_lib(cx: &mut Cx, lib_id: LibId) -> Result<()> {
    publish_organ_claims_for_lib(cx, lib_id, sequence_organ_symbol(), sequence_op_keys())
}

fn sequence_op_key(name: &str) -> OpKey {
    OpKey::new(Symbol::new("sequence"), Symbol::new(name), 1)
}
