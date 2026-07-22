use sim_kernel::{
    Cx, LibId, OpKey, Result, Symbol,
    standard::{publish_organ_claims, publish_organ_claims_for_lib},
};

use crate::let_form::LetForm;

/// Symbol identifying the binding organ in the claim store.
///
/// Used as the claim subject when publishing the organ's contributed
/// operations into a [`Cx`].
pub fn binding_organ_symbol() -> Symbol {
    Symbol::qualified("organ", "binding")
}

/// Operation key for the `let` form (parallel lexical binding).
pub fn binding_let_op_key() -> OpKey {
    binding_op_key("let")
}

/// Operation key for the `let*` form (sequential lexical binding).
pub fn binding_let_star_op_key() -> OpKey {
    binding_op_key("let-star")
}

/// Operation key for the `letrec` form (mutually recursive lexical binding).
pub fn binding_letrec_op_key() -> OpKey {
    binding_op_key("letrec")
}

/// Operation key for the `dynamic-let` form (dynamic-extent binding).
pub fn binding_dynamic_let_op_key() -> OpKey {
    binding_op_key("dynamic-let")
}

/// Operation key for the `parameterize` form (dynamic parameter rebinding).
pub fn binding_parameterize_op_key() -> OpKey {
    binding_op_key("parameterize")
}

/// Operation key for the `profile-modes` form (per-profile binding/hygiene modes).
pub fn binding_profile_modes_op_key() -> OpKey {
    binding_op_key("profile-modes")
}

/// All binding-surface operations this crate models, whether or not they are
/// currently exported as live runtime callables.
pub fn binding_declared_op_keys() -> Vec<OpKey> {
    [
        binding_let_op_key(),
        binding_let_star_op_key(),
        binding_letrec_op_key(),
        binding_dynamic_let_op_key(),
        binding_parameterize_op_key(),
        binding_profile_modes_op_key(),
    ]
    .into()
}

/// Live binding claim-to-export mappings backed by the loaded runtime surface.
pub fn binding_live_ops() -> Vec<(OpKey, Symbol)> {
    vec![(binding_let_op_key(), LetForm::symbol())]
}

/// Operation keys the binding organ currently publishes as live claims.
pub fn binding_op_keys() -> Vec<OpKey> {
    binding_live_ops()
        .into_iter()
        .map(|(op_key, _export_symbol)| op_key)
        .collect()
}

/// Publishes the binding organ's claims and operation keys into a [`Cx`].
///
/// The kernel defines the claim/organ contract; this crate supplies the
/// binding organ's operation set. After publishing, the organ and its ops
/// are discoverable through the standard Card projection.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
/// use sim_lib_binding::publish_binding_organ_claims;
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// publish_binding_organ_claims(&mut cx).unwrap();
/// ```
pub fn publish_binding_organ_claims(cx: &mut Cx) -> Result<()> {
    publish_organ_claims(cx, binding_organ_symbol(), binding_op_keys())
}

/// Publishes binding organ claims as part of a loaded lib receipt.
pub fn publish_binding_organ_claims_for_lib(cx: &mut Cx, lib_id: LibId) -> Result<()> {
    publish_organ_claims_for_lib(cx, lib_id, binding_organ_symbol(), binding_op_keys())
}

fn binding_op_key(name: &str) -> OpKey {
    OpKey::new(Symbol::new("binding"), Symbol::new(name), 1)
}
