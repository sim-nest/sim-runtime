use sim_kernel::Symbol;

/// Runtime role a documented ISLISP surface form lowers to.
///
/// The kernel owns the codec and dispatch contracts; this enum only labels
/// which organ surface a given ISLISP defining form targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IslispFormRole {
    /// Form that declares an ISLISP class or object recipe.
    Object,
    /// Form that declares or extends a generic function.
    Generic,
}

/// Documentation record for one ISLISP defining form.
///
/// Describes how a surface symbol in the ISLISP profile maps onto the shared
/// dispatch organ; it carries metadata only, not the lowering behavior itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IslispFormSpec {
    /// Qualified surface symbol of the form (for example `islisp/defclass`).
    pub symbol: Symbol,
    /// Runtime role the form lowers to.
    pub role: IslispFormRole,
    /// Organ symbol that supplies the form's behavior.
    pub organ: Symbol,
    /// One-line human-readable description of the form.
    pub doc: &'static str,
}

/// Returns the documented ISLISP defining forms supported by this profile.
///
/// Each entry maps a surface symbol onto the shared dispatch organ; see the
/// crate README language-profiles section.
pub fn islisp_form_specs() -> Vec<IslispFormSpec> {
    vec![
        IslispFormSpec {
            symbol: Symbol::qualified("islisp", "defclass"),
            role: IslispFormRole::Object,
            organ: sim_lib_dispatch::dispatch_organ_symbol(),
            doc: "Declare an ISLISP class recipe consumed by generic method shapes.",
        },
        IslispFormSpec {
            symbol: Symbol::qualified("islisp", "defgeneric"),
            role: IslispFormRole::Generic,
            organ: sim_lib_dispatch::dispatch_organ_symbol(),
            doc: "Declare a generic function backed by the shared dispatch organ.",
        },
        IslispFormSpec {
            symbol: Symbol::qualified("islisp", "defmethod"),
            role: IslispFormRole::Generic,
            organ: sim_lib_dispatch::dispatch_organ_symbol(),
            doc: "Attach a primary method to an ISLISP generic through dispatch.",
        },
    ]
}
