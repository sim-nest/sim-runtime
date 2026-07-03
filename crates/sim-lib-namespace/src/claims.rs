//! Organ identity and operation claims for the namespace organ.
//!
//! The kernel owns the claim/fact store and the `OpKey` and organ-claim
//! contracts; this module names the namespace organ and the operation keys it
//! exposes, then publishes them as standard organ claims so the organ is
//! discoverable through the kernel's Card surface. See the crate
//! [`README`](https://docs.rs/sim-runtime) for the constellation framing.

use sim_kernel::{
    Cx, LibId, OpKey, Result, Symbol,
    standard::{publish_organ_claims, publish_organ_claims_for_lib},
};

/// The symbol that identifies the namespace organ in the claim store.
pub fn namespace_organ_symbol() -> Symbol {
    Symbol::qualified("organ", "namespace")
}

/// Operation key for declaring a package namespace.
pub fn namespace_package_op_key() -> OpKey {
    namespace_op_key("package")
}

/// Operation key for declaring a module namespace.
pub fn namespace_module_op_key() -> OpKey {
    namespace_op_key("module")
}

/// Operation key for exporting a binding from a namespace.
pub fn namespace_export_op_key() -> OpKey {
    namespace_op_key("export")
}

/// Operation key for importing an exported binding into a namespace.
pub fn namespace_import_op_key() -> OpKey {
    namespace_op_key("import")
}

/// Operation key for importing a binding under a renamed alias.
pub fn namespace_rename_op_key() -> OpKey {
    namespace_op_key("rename")
}

/// Operation key for importing a binding that shadows an existing one.
pub fn namespace_shadow_op_key() -> OpKey {
    namespace_op_key("shadow")
}

/// The full set of operation keys exposed by the namespace organ.
///
/// Ordered package, module, export, import, rename, shadow; published verbatim
/// by [`publish_namespace_organ_claims`].
pub fn namespace_op_keys() -> Vec<OpKey> {
    [
        namespace_package_op_key(),
        namespace_module_op_key(),
        namespace_export_op_key(),
        namespace_import_op_key(),
        namespace_rename_op_key(),
        namespace_shadow_op_key(),
    ]
    .into()
}

/// Publish the namespace organ and its operation keys as standard organ claims.
///
/// Realizes the kernel's organ-claim contract for the namespace organ: after
/// this call the organ is discoverable through the kernel Card surface keyed by
/// [`namespace_organ_symbol`].
pub fn publish_namespace_organ_claims(cx: &mut Cx) -> Result<()> {
    publish_organ_claims(cx, namespace_organ_symbol(), namespace_op_keys())
}

/// Publish the namespace organ claims as part of a loaded lib receipt.
pub fn publish_namespace_organ_claims_for_lib(cx: &mut Cx, lib_id: LibId) -> Result<()> {
    publish_organ_claims_for_lib(cx, lib_id, namespace_organ_symbol(), namespace_op_keys())
}

fn namespace_op_key(name: &str) -> OpKey {
    OpKey::new(Symbol::new("namespace"), Symbol::new(name), 1)
}
