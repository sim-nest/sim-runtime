use sim_kernel::{Cx, Result, Symbol};
use sim_lib_standard_core::{
    LanguageProfile, OrganUse, ProfileBackingLib, ProfileRegistry, fidelity_badge,
    install_language_profile,
};

use crate::{
    lua_conformance_test_symbol, lua_control_fidelity_symbol, lua_full_runtime_fidelity_symbol,
    lua_lowering_symbol, lua_mutation_fidelity_symbol, lua_profile_symbol, lua_reader_symbol,
};

/// Builds the [`LanguageProfile`] describing the Lua core surface profile.
///
/// Reuses the shared algol reader, wires Lua lowering and eval policy, draws on
/// the control and mutation organs, requires the standard mutate capability,
/// marks full-VM debug hooks as unsupported, and publishes honest fidelity
/// badges (coroutines and table mutation supported, full runtime limited).
pub fn lua_core_profile() -> LanguageProfile {
    let profile = lua_profile_symbol();
    let test = lua_conformance_test_symbol();
    LanguageProfile::new(profile.clone())
        .with_reader(lua_reader_symbol())
        .with_lowering(lua_lowering_symbol())
        .with_eval_policy(Symbol::qualified("eval", "lua-core"))
        .with_organ(OrganUse::new(sim_lib_control::control_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_mutation::mutation_organ_symbol()))
        .requiring(sim_lib_mutation::standard_mutate_capability())
        .with_unsupported_form(Symbol::qualified("lua", "full-vm-debug-hooks"))
        .with_conformance_test(test.clone())
        .with_fidelity_badge(fidelity_badge(
            &profile,
            lua_control_fidelity_symbol(),
            1,
            &test,
        ))
        .with_fidelity_badge(fidelity_badge(
            &profile,
            lua_mutation_fidelity_symbol(),
            1,
            &test,
        ))
        .with_fidelity_badge(fidelity_badge(
            &profile,
            lua_full_runtime_fidelity_symbol(),
            0,
            &test,
        ))
}

/// Installs the Lua core profile and its organ claims into a registry.
///
/// First-reach entry point: registers [`lua_core_profile`] through the standard
/// profile installer, publishing the control- and mutation-organ claims so the
/// surface becomes loadable.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Symbol};
/// use sim_lib_standard_core::ProfileRegistry;
/// use sim_lib_lang_lua::install_lua_core_profile;
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let mut registry = ProfileRegistry::new();
/// let profile = install_lua_core_profile(&mut cx, &mut registry).unwrap();
/// assert_eq!(profile.reader, Symbol::qualified("codec", "algol"));
/// ```
pub fn install_lua_core_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
) -> Result<LanguageProfile> {
    install_language_profile(
        cx,
        registry,
        lua_core_profile(),
        &[
            ProfileBackingLib::loadable(
                sim_lib_control::control_organ_symbol(),
                sim_lib_control::manifest_name(),
                sim_lib_control::install_control_lib,
                None,
            ),
            ProfileBackingLib::unresolved(
                sim_lib_mutation::mutation_organ_symbol(),
                Symbol::qualified("sim", "mutation"),
            ),
        ],
        &[],
    )
}
