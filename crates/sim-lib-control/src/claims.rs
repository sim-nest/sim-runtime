use sim_kernel::{
    Cx, LibId, OpKey, Result, Symbol,
    standard::{publish_organ_claims, publish_organ_claims_for_lib, standard_control_op_key},
};

/// Returns the `organ/control` symbol identifying this control organ.
pub fn control_organ_symbol() -> Symbol {
    Symbol::qualified("organ", "control")
}

/// Returns the standard control operation keys this organ claims:
/// `prompt`, `capture`, `abort`, and `resume`.
pub fn control_op_keys() -> Vec<OpKey> {
    ["prompt", "capture", "abort", "resume"]
        .into_iter()
        .map(standard_control_op_key)
        .collect()
}

/// Publishes the control organ's claims into `cx`, recording that this organ
/// supplies the standard [`control_op_keys`] under [`control_organ_symbol`].
pub fn publish_control_organ_claims(cx: &mut Cx) -> Result<()> {
    publish_organ_claims(cx, control_organ_symbol(), control_op_keys())
}

/// Publishes control organ claims as part of a loaded lib receipt.
pub fn publish_control_organ_claims_for_lib(cx: &mut Cx, lib_id: LibId) -> Result<()> {
    publish_organ_claims_for_lib(cx, lib_id, control_organ_symbol(), control_op_keys())
}
