use sim_kernel::{
    Cx, LibId, OpKey, Result, Symbol,
    standard::{publish_organ_claims, publish_organ_claims_for_lib},
};

/// The organ symbol under which this crate publishes its claims: `organ:mutation`.
pub fn mutation_organ_symbol() -> Symbol {
    Symbol::qualified("organ", "mutation")
}

/// Operation key for constructing a mutable cell (`mutation/cell`).
pub fn mutation_cell_op_key() -> OpKey {
    mutation_op_key("cell")
}

/// Operation key for constructing a mutable box (`mutation/box`).
pub fn mutation_box_op_key() -> OpKey {
    mutation_op_key("box")
}

/// Operation key for an in-place write (`mutation/set`).
pub fn mutation_set_op_key() -> OpKey {
    mutation_op_key("set")
}

/// Operation key for constructing a mutable vector (`mutation/vector`).
pub fn mutation_vector_op_key() -> OpKey {
    mutation_op_key("vector")
}

/// Operation key for constructing a mutable table (`mutation/table`).
pub fn mutation_table_op_key() -> OpKey {
    mutation_op_key("table")
}

/// The full set of mutation operation keys this organ exposes.
///
/// Ordered cell, box, set, vector, table; passed to
/// [`publish_mutation_organ_claims`] when the organ registers its claims.
pub fn mutation_op_keys() -> Vec<OpKey> {
    [
        mutation_cell_op_key(),
        mutation_box_op_key(),
        mutation_set_op_key(),
        mutation_vector_op_key(),
        mutation_table_op_key(),
    ]
    .into()
}

/// Publish the mutation organ and its operation keys as claims into `cx`.
///
/// Registers [`mutation_organ_symbol`] as a standard organ carrying
/// [`mutation_op_keys`], making the organ discoverable through the kernel card
/// and claim surfaces.
pub fn publish_mutation_organ_claims(cx: &mut Cx) -> Result<()> {
    publish_organ_claims(cx, mutation_organ_symbol(), mutation_op_keys())
}

/// Publish the mutation organ claims as part of a loaded lib receipt.
pub fn publish_mutation_organ_claims_for_lib(cx: &mut Cx, lib_id: LibId) -> Result<()> {
    publish_organ_claims_for_lib(cx, lib_id, mutation_organ_symbol(), mutation_op_keys())
}

fn mutation_op_key(name: &str) -> OpKey {
    OpKey::new(Symbol::new("mutation"), Symbol::new(name), 1)
}
