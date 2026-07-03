//! Installing language profiles into a profile registry and publishing claims.

use sim_kernel::{Cx, OpKey, Result, Symbol};

use crate::{
    LanguageProfile, ProfileRegistry, install_language_profile, standard_install_capability,
};

/// Summary returned by [`install_profile_stub`]: the installed profile symbol
/// and its organ and conformance-test counts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandardInstallReport {
    /// Symbol of the installed profile.
    pub profile: Symbol,
    /// Number of organs the profile uses.
    pub organ_count: usize,
    /// Number of conformance tests the profile declares.
    pub test_count: usize,
}

/// Operation key for the standard install operation.
pub fn standard_install_op_key() -> OpKey {
    OpKey::new(Symbol::new("standard"), Symbol::new("install"), 1)
}

/// Install `profile` into `registry` and publish its claims, gated on
/// [`standard_install_capability`].
///
/// This is a first-reach entry point: it registers the profile, publishes its
/// profile and badge claims, and returns a [`StandardInstallReport`].
///
/// [`standard_install_capability`]: crate::standard_install_capability
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
///
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Symbol};
/// use sim_lib_standard_core::{
///     LanguageProfile, ProfileRegistry, install_profile_stub, standard_install_capability,
/// };
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// cx.grant(standard_install_capability());
/// let mut registry = ProfileRegistry::new();
///
/// let profile = LanguageProfile::new(Symbol::qualified("lang", "demo/v1"))
///     .with_reader(Symbol::qualified("codec", "lisp"));
/// let report = install_profile_stub(&mut cx, &mut registry, profile).unwrap();
///
/// assert_eq!(report.profile, Symbol::qualified("lang", "demo/v1"));
/// assert!(registry.profile(&Symbol::qualified("lang", "demo/v1")).is_some());
/// ```
pub fn install_profile_stub(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
    profile: LanguageProfile,
) -> Result<StandardInstallReport> {
    cx.require(&standard_install_capability())?;
    let report = StandardInstallReport {
        profile: profile.symbol.clone(),
        organ_count: profile.organs.len(),
        test_count: profile.conformance_tests.len(),
    };
    install_language_profile(cx, registry, profile, &[])?;
    Ok(report)
}
