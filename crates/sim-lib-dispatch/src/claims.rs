use sim_kernel::{
    Cx, LibId, OpKey, Result, Symbol,
    standard::{publish_organ_claims, publish_organ_claims_for_lib},
};

/// Symbol identifying the dispatch organ in the claim store.
///
/// Used as the claim subject when publishing the organ's contributed
/// operations into a [`Cx`].
pub fn dispatch_organ_symbol() -> Symbol {
    Symbol::qualified("organ", "dispatch")
}

/// Operation key for defining a generic function.
pub fn dispatch_generic_op_key() -> OpKey {
    dispatch_op_key("generic")
}

/// Operation key for adding a method to a generic function (multimethod).
pub fn dispatch_multimethod_op_key() -> OpKey {
    dispatch_op_key("multimethod")
}

/// Operation key for method combination (around/before/primary/after).
pub fn dispatch_combine_op_key() -> OpKey {
    dispatch_op_key("combine")
}

/// Operation key for reporting method specificity for a call.
pub fn dispatch_specificity_op_key() -> OpKey {
    dispatch_op_key("specificity")
}

/// Operation key for inspecting the applicable methods of a call.
pub fn dispatch_inspect_op_key() -> OpKey {
    dispatch_op_key("inspect")
}

/// All operation keys contributed by the dispatch organ, in claim order.
pub fn dispatch_op_keys() -> Vec<OpKey> {
    [
        dispatch_generic_op_key(),
        dispatch_multimethod_op_key(),
        dispatch_combine_op_key(),
        dispatch_specificity_op_key(),
        dispatch_inspect_op_key(),
    ]
    .into()
}

/// Publishes the dispatch organ's claims and operation keys into a [`Cx`].
///
/// The kernel defines the claim/organ contract; this crate supplies the
/// dispatch organ's operation set. After publishing, the organ and its ops
/// are discoverable through the standard Card projection.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
/// use sim_lib_dispatch::publish_dispatch_organ_claims;
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// publish_dispatch_organ_claims(&mut cx).unwrap();
/// ```
pub fn publish_dispatch_organ_claims(cx: &mut Cx) -> Result<()> {
    publish_organ_claims(cx, dispatch_organ_symbol(), dispatch_op_keys())
}

/// Publishes dispatch organ claims as part of a loaded lib receipt.
pub fn publish_dispatch_organ_claims_for_lib(cx: &mut Cx, lib_id: LibId) -> Result<()> {
    publish_organ_claims_for_lib(cx, lib_id, dispatch_organ_symbol(), dispatch_op_keys())
}

fn dispatch_op_key(name: &str) -> OpKey {
    OpKey::new(Symbol::new("dispatch"), Symbol::new(name), 1)
}
