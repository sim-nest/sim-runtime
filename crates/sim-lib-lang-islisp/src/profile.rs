use sim_kernel::{Cx, Ref, Result, Symbol};
use sim_lib_standard_core::{
    FidelityBadge, LanguageProfile, OrganUse, ProfileRegistry, install_language_profile,
};

use crate::{
    islisp_conformance_test_symbol, islisp_dispatch_fidelity_symbol, islisp_lowering_symbol,
    islisp_profile_symbol, islisp_reader_symbol,
};

/// Builds the [`LanguageProfile`] describing the ISLISP surface profile.
///
/// Wires the ISLISP reader, lowering, eval policy, numeric tower, dispatch
/// organ, and conformance fidelity badge as data; it describes the profile but
/// installs nothing on its own.
pub fn islisp_profile() -> LanguageProfile {
    let profile = islisp_profile_symbol();
    let test = islisp_conformance_test_symbol();
    LanguageProfile::new(profile.clone())
        .with_reader(islisp_reader_symbol())
        .with_lowering(islisp_lowering_symbol())
        .with_eval_policy(Symbol::qualified("eval", "islisp-core"))
        .with_organ(OrganUse::new(sim_lib_dispatch::dispatch_organ_symbol()))
        .with_numeric_tower(Symbol::qualified("numbers", "islisp-core"))
        .with_conformance_test(test.clone())
        .with_fidelity_badge(FidelityBadge::new(
            Ref::Symbol(profile),
            islisp_dispatch_fidelity_symbol(),
            1,
            Ref::Symbol(test),
        ))
}

/// Installs the ISLISP profile and its dispatch-organ claims into a registry.
///
/// First-reach entry point: registers [`islisp_profile`] through the standard
/// profile installer so the surface becomes loadable.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
/// use sim_lib_standard_core::ProfileRegistry;
/// use sim_lib_lang_islisp::install_islisp_profile;
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let mut registry = ProfileRegistry::new();
/// let profile = install_islisp_profile(&mut cx, &mut registry).unwrap();
/// assert!(
///     profile
///         .organs
///         .iter()
///         .any(|organ| organ.organ == sim_lib_dispatch::dispatch_organ_symbol())
/// );
/// ```
pub fn install_islisp_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
) -> Result<LanguageProfile> {
    install_language_profile(
        cx,
        registry,
        islisp_profile(),
        &[sim_lib_dispatch::publish_dispatch_organ_claims_for_lib],
    )
}
