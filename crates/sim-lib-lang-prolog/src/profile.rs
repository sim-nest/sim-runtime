use sim_kernel::{Cx, Result, Symbol};
use sim_lib_standard_core::{
    LanguageProfile, OrganUse, ProfileRegistry, fidelity_badge, install_language_profile,
};

use crate::{
    install_prolog_lib, prolog_conformance_test_symbol, prolog_logic_organ_symbol,
    prolog_lowering_symbol, prolog_profile_symbol, prolog_reader_symbol,
    prolog_surface_fidelity_symbol,
};

/// Describes the Prolog surface profile as standard-distribution data.
///
/// The profile uses Lisp-expression input for asserted clauses and goals,
/// installs the logic eval policy, draws on the logic, sequence, and control
/// organs, and reports level 1 surface fidelity for the current organ-backed
/// builtin coverage.
pub fn prolog_profile() -> LanguageProfile {
    let profile = prolog_profile_symbol();
    let test = prolog_conformance_test_symbol();
    LanguageProfile::new(profile.clone())
        .with_reader(prolog_reader_symbol())
        .with_lowering(prolog_lowering_symbol())
        .with_eval_policy(Symbol::qualified("eval", "logic"))
        .with_organ(OrganUse::new(prolog_logic_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_sequence::sequence_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_control::control_organ_symbol()))
        .with_conformance_test(test.clone())
        .with_fidelity_badge(fidelity_badge(
            &profile,
            prolog_surface_fidelity_symbol(),
            1,
            &test,
        ))
}

/// Installs the Prolog profile into a [`ProfileRegistry`].
///
/// The Prolog lib is loaded first so the profile symbol and callable surface are
/// available together in the same context.
pub fn install_prolog_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
) -> Result<LanguageProfile> {
    install_prolog_lib(cx)?;
    install_language_profile(
        cx,
        registry,
        prolog_profile(),
        &[
            sim_lib_sequence::publish_sequence_organ_claims_for_lib,
            sim_lib_control::publish_control_organ_claims_for_lib,
        ],
    )
}

#[cfg(test)]
mod tests {
    use sim_kernel::{
        ClaimPattern, Ref, Symbol, card::card_kind_predicate, standard::standard_profile_kind,
        testing::bare_cx as cx,
    };

    use super::*;
    use crate::prolog_surface_fidelity_symbol;

    #[test]
    fn prolog_profile_installs_with_level_one_fidelity_claim() {
        let mut cx = cx();
        let mut registry = ProfileRegistry::new();

        let profile = install_prolog_profile(&mut cx, &mut registry).unwrap();

        assert_eq!(profile.symbol, prolog_profile_symbol());
        assert!(
            profile
                .organs
                .iter()
                .any(|organ| organ.organ == sim_lib_sequence::sequence_organ_symbol())
        );
        assert!(cx.registry().lib(&Symbol::new("prolog")).is_some());
        assert!(registry.profile(&prolog_profile_symbol()).is_some());
        let profile_kind = cx.query_facts(profile_kind_claim()).unwrap();
        assert_eq!(profile_kind.len(), 1);
        let fidelity = cx.query_facts(partial_fidelity_claim()).unwrap();
        assert_eq!(fidelity.len(), 1);
    }

    fn profile_kind_claim() -> ClaimPattern {
        ClaimPattern {
            subject: Some(Ref::Symbol(prolog_profile_symbol())),
            predicate: Some(card_kind_predicate()),
            object: Some(Ref::Symbol(standard_profile_kind())),
            include_revoked: false,
        }
    }

    fn partial_fidelity_claim() -> ClaimPattern {
        ClaimPattern {
            subject: Some(Ref::Symbol(prolog_profile_symbol())),
            predicate: Some(Symbol::qualified("standard", "fidelity-badge")),
            object: Some(Ref::Symbol(prolog_surface_fidelity_symbol())),
            include_revoked: false,
        }
    }
}
