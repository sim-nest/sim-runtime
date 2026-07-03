use sim_kernel::{Cx, Result, Symbol};
use sim_lib_standard_core::{
    LanguageProfile, OrganUse, ProfileRegistry, fidelity_badge, install_language_profile,
};

use crate::{
    julia_conformance_test_symbol, julia_dispatch_fidelity_symbol,
    julia_full_runtime_fidelity_symbol, julia_lowering_symbol, julia_profile_symbol,
    julia_reader_symbol,
};

/// Builds the [`LanguageProfile`] describing the Julia core surface profile.
///
/// Reuses the shared algol reader, wires Julia lowering, eval policy, numeric
/// tower, and the dispatch organ, marks world-age runtime as unsupported, and
/// publishes honest fidelity badges (dispatch supported, full runtime limited).
pub fn julia_core_profile() -> LanguageProfile {
    let profile = julia_profile_symbol();
    let test = julia_conformance_test_symbol();
    LanguageProfile::new(profile.clone())
        .with_reader(julia_reader_symbol())
        .with_lowering(julia_lowering_symbol())
        .with_eval_policy(Symbol::qualified("eval", "julia-core"))
        .with_organ(OrganUse::new(sim_lib_dispatch::dispatch_organ_symbol()))
        .with_numeric_tower(Symbol::qualified("numbers", "julia-core"))
        .with_unsupported_form(Symbol::qualified("julia", "world-age-runtime"))
        .with_conformance_test(test.clone())
        .with_fidelity_badge(fidelity_badge(
            &profile,
            julia_dispatch_fidelity_symbol(),
            1,
            &test,
        ))
        .with_fidelity_badge(fidelity_badge(
            &profile,
            julia_full_runtime_fidelity_symbol(),
            0,
            &test,
        ))
}

/// Installs the Julia core profile and its dispatch-organ claims into a registry.
///
/// First-reach entry point: registers [`julia_core_profile`] through the
/// standard profile installer so the surface becomes loadable.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Symbol};
/// use sim_lib_standard_core::ProfileRegistry;
/// use sim_lib_lang_julia::install_julia_core_profile;
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let mut registry = ProfileRegistry::new();
/// let profile = install_julia_core_profile(&mut cx, &mut registry).unwrap();
/// assert_eq!(profile.reader, Symbol::qualified("codec", "algol"));
/// ```
pub fn install_julia_core_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
) -> Result<LanguageProfile> {
    install_language_profile(
        cx,
        registry,
        julia_core_profile(),
        &[sim_lib_dispatch::publish_dispatch_organ_claims_for_lib],
    )
}
