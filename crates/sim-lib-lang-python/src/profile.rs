use sim_kernel::{Cx, Result, Symbol};
use sim_lib_standard_core::{
    LanguageProfile, OrganUse, ProfileBackingLib, ProfileRegistry, install_language_profile,
};

/// Stable symbol for the thin Python core profile.
pub fn python_profile_symbol() -> Symbol {
    Symbol::qualified("lang", "python-core/v1")
}
fn reader() -> Symbol {
    Symbol::qualified("codec", "python")
}
fn lowering() -> Symbol {
    Symbol::qualified("python", "token-lowering/v1")
}
fn eval_policy() -> Symbol {
    Symbol::qualified("python", "direct-eval/v1")
}

/// Build the Python core profile, including capabilities and inspectable gaps.
pub fn python_core_profile() -> LanguageProfile {
    LanguageProfile::new(python_profile_symbol())
        .with_reader(reader())
        .with_lowering(lowering())
        .with_eval_policy(eval_policy())
        .with_organ(OrganUse::new(sim_lib_binding::binding_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_control::control_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_mutation::mutation_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_sequence::sequence_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_dispatch::dispatch_organ_symbol()))
        .requiring(sim_lib_mutation::standard_mutate_capability())
        .with_unsupported_form(Symbol::qualified("python", "bytecode"))
        .with_unsupported_form(Symbol::qualified("python", "foreign-runtime"))
        .with_unsupported_form(Symbol::qualified("python", "annotation-check-pass"))
        .with_unsupported_form(Symbol::qualified("python", "retain-policy-leaks-cycles"))
}

/// Install the profile. Collection is a runtime policy option, never a load prerequisite.
pub fn install_python_core_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
) -> Result<LanguageProfile> {
    install_language_profile(
        cx,
        registry,
        python_core_profile(),
        &[
            ProfileBackingLib::loadable(
                sim_lib_binding::binding_organ_symbol(),
                sim_lib_binding::manifest_name(),
                sim_lib_binding::install_binding_lib,
                Some(sim_lib_binding::publish_binding_organ_claims_for_lib),
            ),
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
            ProfileBackingLib::loadable(
                sim_lib_sequence::sequence_organ_symbol(),
                sim_lib_sequence::manifest_name(),
                sim_lib_sequence::install_sequence_lib,
                Some(sim_lib_sequence::publish_sequence_organ_claims_for_lib),
            ),
            ProfileBackingLib::unresolved(
                sim_lib_dispatch::dispatch_organ_symbol(),
                Symbol::qualified("sim", "dispatch"),
            ),
        ],
        &[],
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_declares_complete_runtime_evidence() {
        let evidence = python_core_profile().checked_guest_evidence().unwrap();
        assert_eq!(evidence.reader, reader());
        assert_eq!(evidence.organs.len(), 5);
        assert_eq!(evidence.capabilities.len(), 1);
        assert_eq!(evidence.gaps.len(), 4);
    }
}
