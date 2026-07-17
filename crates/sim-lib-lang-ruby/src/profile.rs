use sim_kernel::{Cx, Result, Symbol};
use sim_lib_standard_core::{
    LanguageProfile, OrganUse, ProfileBackingLib, ProfileRegistry, fidelity_badge,
    install_language_profile,
};

use crate::{
    ruby_blocks_fidelity_symbol, ruby_conformance_test_symbol, ruby_control_fidelity_symbol,
    ruby_dispatch_fidelity_symbol, ruby_lowering_symbol, ruby_profile_symbol, ruby_reader_symbol,
};

/// Builds the [`LanguageProfile`] describing the Ruby DSL surface profile.
///
/// Reuses the shared algol reader, wires Ruby lowering and eval policy, draws on
/// the control and dispatch organs, marks the full object model as unsupported,
/// and publishes honest fidelity badges (control and dispatch supported, full
/// blocks limited).
pub fn ruby_dsl_profile() -> LanguageProfile {
    let profile = ruby_profile_symbol();
    let test = ruby_conformance_test_symbol();
    LanguageProfile::new(profile.clone())
        .with_reader(ruby_reader_symbol())
        .with_lowering(ruby_lowering_symbol())
        .with_eval_policy(Symbol::qualified("eval", "ruby-dsl"))
        .with_organ(OrganUse::new(sim_lib_control::control_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_dispatch::dispatch_organ_symbol()))
        .with_unsupported_form(Symbol::qualified("ruby", "full-object-model"))
        .with_conformance_test(test.clone())
        .with_fidelity_badge(fidelity_badge(
            &profile,
            ruby_control_fidelity_symbol(),
            1,
            &test,
        ))
        .with_fidelity_badge(fidelity_badge(
            &profile,
            ruby_dispatch_fidelity_symbol(),
            1,
            &test,
        ))
        .with_fidelity_badge(fidelity_badge(
            &profile,
            ruby_blocks_fidelity_symbol(),
            0,
            &test,
        ))
}

/// Installs the Ruby DSL profile and its organ claims into a registry.
///
/// First-reach entry point: registers [`ruby_dsl_profile`] through the standard
/// profile installer, publishing the control- and dispatch-organ claims so the
/// surface becomes loadable.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Symbol};
/// use sim_lib_standard_core::ProfileRegistry;
/// use sim_lib_lang_ruby::install_ruby_dsl_profile;
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let mut registry = ProfileRegistry::new();
/// let profile = install_ruby_dsl_profile(&mut cx, &mut registry).unwrap();
/// assert_eq!(profile.reader, Symbol::qualified("codec", "algol"));
/// ```
pub fn install_ruby_dsl_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
) -> Result<LanguageProfile> {
    install_language_profile(
        cx,
        registry,
        ruby_dsl_profile(),
        &[
            ProfileBackingLib::loadable(
                sim_lib_control::control_organ_symbol(),
                sim_lib_control::manifest_name(),
                sim_lib_control::install_control_lib,
                None,
            ),
            ProfileBackingLib::unresolved(
                sim_lib_dispatch::dispatch_organ_symbol(),
                Symbol::qualified("sim", "dispatch"),
            ),
        ],
        &[],
    )
}
