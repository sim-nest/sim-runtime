use sim_kernel::{Cx, Ref, Result, Symbol};
use sim_lib_standard_core::{
    FidelityBadge, LanguageProfile, OrganUse, ProfileBackingLib, ProfileRegistry,
    install_language_profile,
};

use crate::{
    clojure_conformance_test_symbol, clojure_control_fidelity_symbol, clojure_core_profile_symbol,
    clojure_edn_reader_symbol, clojure_lowering_symbol, clojure_namespace_fidelity_symbol,
    clojure_sequence_fidelity_symbol,
};

/// Describes the Clojure-core language profile as standard-distribution data.
///
/// Binds the EDN reader, lowering, eval policy, numeric tower, and the sequence,
/// namespace, and control organs, with a per-organ [`FidelityBadge`] against the
/// conformance test.
pub fn clojure_core_profile() -> LanguageProfile {
    let profile = clojure_core_profile_symbol();
    let test = clojure_conformance_test_symbol();
    LanguageProfile::new(profile.clone())
        .with_reader(clojure_edn_reader_symbol())
        .with_lowering(clojure_lowering_symbol())
        .with_eval_policy(Symbol::qualified("eval", "clojure-core"))
        .with_organ(OrganUse::new(sim_lib_sequence::sequence_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_namespace::namespace_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_control::control_organ_symbol()))
        .with_numeric_tower(Symbol::qualified("numbers", "clojure-core"))
        .with_conformance_test(test.clone())
        .with_fidelity_badge(FidelityBadge::new(
            Ref::Symbol(profile.clone()),
            clojure_sequence_fidelity_symbol(),
            1,
            Ref::Symbol(test.clone()),
        ))
        .with_fidelity_badge(FidelityBadge::new(
            Ref::Symbol(profile.clone()),
            clojure_namespace_fidelity_symbol(),
            1,
            Ref::Symbol(test.clone()),
        ))
        .with_fidelity_badge(FidelityBadge::new(
            Ref::Symbol(profile),
            clojure_control_fidelity_symbol(),
            1,
            Ref::Symbol(test),
        ))
}

/// Installs the Clojure-core profile into a [`ProfileRegistry`], publishing organ claims.
///
/// Registers [`clojure_core_profile`] and runs the sequence, namespace, and
/// control organ claim publishers as a side effect.
pub fn install_clojure_core_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
) -> Result<LanguageProfile> {
    install_language_profile(
        cx,
        registry,
        clojure_core_profile(),
        &[
            ProfileBackingLib::loadable(
                sim_lib_sequence::sequence_organ_symbol(),
                Symbol::qualified("sim", "sequence"),
                sim_lib_sequence::install_sequence_lib,
                Some(sim_lib_sequence::publish_sequence_organ_claims_for_lib),
            ),
            ProfileBackingLib::unresolved(
                sim_lib_namespace::namespace_organ_symbol(),
                Symbol::qualified("sim", "namespace"),
            ),
            ProfileBackingLib::loadable(
                sim_lib_control::control_organ_symbol(),
                sim_lib_control::manifest_name(),
                sim_lib_control::install_control_lib,
                None,
            ),
        ],
        &[],
    )
}
