use sim_kernel::{Cx, Result, Symbol};
use sim_lib_standard_core::{
    LanguageProfile, OrganUse, ProfileRegistry, fidelity_badge, install_language_profile,
};

use crate::{
    typed_lazy_conformance_test_symbol, typed_lazy_control_fidelity_symbol,
    typed_lazy_lowering_symbol, typed_lazy_pattern_fidelity_symbol, typed_lazy_profile_symbol,
    typed_lazy_reader_symbol, typed_lazy_typeclass_fidelity_symbol,
};

/// Describes the typed, lazy language profile as standard-distribution data.
///
/// Binds the reader, lowering, eval policy, and the pattern and control organs.
/// Full type inference is declared unsupported, and the laziness and typeclass
/// fidelity badges are level 0 to mark those surfaces as limited.
pub fn typed_lazy_profile() -> LanguageProfile {
    let profile = typed_lazy_profile_symbol();
    let test = typed_lazy_conformance_test_symbol();
    LanguageProfile::new(profile.clone())
        .with_reader(typed_lazy_reader_symbol())
        .with_lowering(typed_lazy_lowering_symbol())
        .with_eval_policy(Symbol::qualified("eval", "typed-lazy"))
        .with_organ(OrganUse::new(sim_lib_pattern::pattern_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_control::control_organ_symbol()))
        .with_unsupported_form(Symbol::qualified("typed-lazy", "full-type-inference"))
        .with_conformance_test(test.clone())
        .with_fidelity_badge(fidelity_badge(
            &profile,
            typed_lazy_pattern_fidelity_symbol(),
            1,
            &test,
        ))
        .with_fidelity_badge(fidelity_badge(
            &profile,
            typed_lazy_control_fidelity_symbol(),
            0,
            &test,
        ))
        .with_fidelity_badge(fidelity_badge(
            &profile,
            typed_lazy_typeclass_fidelity_symbol(),
            0,
            &test,
        ))
}

/// Installs the typed-lazy profile into a [`ProfileRegistry`], publishing organ claims.
///
/// Registers [`typed_lazy_profile`] and runs the pattern and control organ claim
/// publishers as a side effect.
pub fn install_typed_lazy_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
) -> Result<LanguageProfile> {
    install_language_profile(
        cx,
        registry,
        typed_lazy_profile(),
        &[
            sim_lib_pattern::publish_pattern_organ_claims_for_lib,
            sim_lib_control::publish_control_organ_claims_for_lib,
        ],
    )
}
