//! Capability names for the standard distribution (install, diff, test).

use sim_kernel::CapabilityName;

/// Capability gating profile install ([`install_profile_stub`]).
///
/// [`install_profile_stub`]: crate::install_profile_stub
pub fn standard_install_capability() -> CapabilityName {
    CapabilityName::new("standard.install")
}

/// Capability gating profile diffing ([`standard_diff_stub`]).
///
/// [`standard_diff_stub`]: crate::standard_diff_stub
pub fn standard_diff_capability() -> CapabilityName {
    CapabilityName::new("standard.diff")
}

/// Capability gating the conformance harness ([`standard_test_stub`]).
///
/// [`standard_test_stub`]: crate::standard_test_stub
pub fn standard_test_capability() -> CapabilityName {
    CapabilityName::new("standard.test")
}
