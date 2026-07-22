use sim_kernel::{Cx, Result, Symbol};
use sim_lib_standard_core::{
    LanguageProfile, OrganUse, ProfileBackingLib, ProfileRegistry, fidelity_badge,
    install_language_profile,
};

use crate::{
    cl_binding_fidelity_symbol, cl_clos_mop_fidelity_symbol, cl_conformance_test_symbol,
    cl_control_fidelity_symbol, cl_dispatch_fidelity_symbol, cl_lite_profile_symbol,
    cl_lowering_symbol, cl_mutation_fidelity_symbol, cl_namespace_fidelity_symbol,
    cl_reader_symbol,
};

/// Builds the CL-lite [`LanguageProfile`]: reader, lowering, eval policy, the
/// five backing organs (binding, control, dispatch, namespace, mutation),
/// numeric tower, required mutation capability, conformance test, and per-organ
/// fidelity badges.
///
/// The CLOS/MOP badge is level 0 and `full-clos-mop` is listed as an unsupported
/// form: the profile presents a limited CLOS surface. See the crate [README]
/// for the language-profile role.
///
/// [README]: https://docs.rs/crate/sim-lib-lang-cl
pub fn cl_lite_profile() -> LanguageProfile {
    let profile = cl_lite_profile_symbol();
    let test = cl_conformance_test_symbol();
    LanguageProfile::new(profile.clone())
        .with_reader(cl_reader_symbol())
        .with_lowering(cl_lowering_symbol())
        .with_eval_policy(Symbol::qualified("eval", "common-lisp-lite"))
        .with_organ(OrganUse::new(sim_lib_binding::binding_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_control::control_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_dispatch::dispatch_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_namespace::namespace_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_mutation::mutation_organ_symbol()))
        .with_numeric_tower(Symbol::qualified("numbers", "common-lisp-lite"))
        .requiring(sim_lib_mutation::standard_mutate_capability())
        .with_unsupported_form(Symbol::qualified("cl", "full-clos-mop"))
        .with_conformance_test(test.clone())
        .with_fidelity_badge(fidelity_badge(
            &profile,
            cl_binding_fidelity_symbol(),
            1,
            &test,
        ))
        .with_fidelity_badge(fidelity_badge(
            &profile,
            cl_control_fidelity_symbol(),
            1,
            &test,
        ))
        .with_fidelity_badge(fidelity_badge(
            &profile,
            cl_dispatch_fidelity_symbol(),
            1,
            &test,
        ))
        .with_fidelity_badge(fidelity_badge(
            &profile,
            cl_namespace_fidelity_symbol(),
            1,
            &test,
        ))
        .with_fidelity_badge(fidelity_badge(
            &profile,
            cl_mutation_fidelity_symbol(),
            1,
            &test,
        ))
        .with_fidelity_badge(fidelity_badge(
            &profile,
            cl_clos_mop_fidelity_symbol(),
            0,
            &test,
        ))
}

/// Installs the CL-lite profile into a [`ProfileRegistry`], publishing the
/// backing organ claims for binding, control, dispatch, namespace, and mutation.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
/// use sim_lib_standard_core::ProfileRegistry;
/// use sim_lib_lang_cl::install_cl_lite_profile;
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let mut registry = ProfileRegistry::new();
/// let profile = install_cl_lite_profile(&mut cx, &mut registry).unwrap();
/// assert!(registry.profile(&profile.symbol).is_some());
/// ```
pub fn install_cl_lite_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
) -> Result<LanguageProfile> {
    install_language_profile(
        cx,
        registry,
        cl_lite_profile(),
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
                sim_lib_dispatch::dispatch_organ_symbol(),
                Symbol::qualified("sim", "dispatch"),
            ),
            ProfileBackingLib::unresolved(
                sim_lib_namespace::namespace_organ_symbol(),
                Symbol::qualified("sim", "namespace"),
            ),
            ProfileBackingLib::unresolved(
                sim_lib_mutation::mutation_organ_symbol(),
                Symbol::qualified("sim", "mutation"),
            ),
        ],
        &[],
    )
}
