//! Shared language-profile helpers.
//!
//! Every per-language crate (`sim-lib-lang-*`) builds a [`LanguageProfile`],
//! attaches [`FidelityBadge`]s to it, and installs it with the same fixed
//! sequence: register the profile, publish its profile claims, then publish the
//! organ claims for each organ the profile uses. These helpers capture that
//! shared shape so the per-crate code only carries the language-specific
//! definitions (the profile builder and its organ publishers).

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Cx, Export, Lib, LibId, LibManifest, LibTarget, Linker, Ref, Result, Symbol,
    Version,
};

use crate::{
    FidelityBadge, LanguageProfile, LanguageProfileValue, ProfileRegistry,
    publish_profile_claims_for_lib,
};

/// Organ claim publisher used by recorded profile installs.
pub type ProfileOrganPublisher = fn(&mut Cx, LibId) -> Result<()>;

/// Build a [`FidelityBadge`] for `profile` from a badge symbol, level, and the
/// conformance `test` that evidences it.
///
/// This is the shared body for the per-language `profile_badge` helper.
pub fn fidelity_badge(profile: &Symbol, badge: Symbol, level: u8, test: &Symbol) -> FidelityBadge {
    FidelityBadge::new(
        Ref::Symbol(profile.clone()),
        badge,
        level,
        Ref::Symbol(test.clone()),
    )
}

/// Data form of a fidelity badge: a badge symbol, its level, and the
/// conformance test that evidences it. Resolve it against a profile symbol with
/// [`FidelityBadgeSpec::into_badge`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FidelityBadgeSpec {
    /// The badge name.
    pub badge: Symbol,
    /// Fidelity level, higher meaning more faithful.
    pub level: u8,
    /// Conformance test that evidences the badge.
    pub test: Symbol,
}

impl FidelityBadgeSpec {
    /// Resolve this spec into a [`FidelityBadge`] for `profile`.
    pub fn into_badge(self, profile: &Symbol) -> FidelityBadge {
        fidelity_badge(profile, self.badge, self.level, &self.test)
    }
}

/// Install a fully-built [`LanguageProfile`]: register it, publish its profile
/// claims, then run each organ-claim publisher in order.
///
/// This is the exact install sequence every `install_*_dsl_profile` repeats.
/// Organ publishers are passed in because they are organ-specific; they must be
/// listed in the same order the profile lists its organs.
pub fn install_language_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
    profile: LanguageProfile,
    organ_publishers: &[ProfileOrganPublisher],
) -> Result<LanguageProfile> {
    let lib_id = ensure_profile_lib(cx, &profile)?;
    registry.register_profile(profile.clone())?;
    publish_profile_claims_for_lib(cx, lib_id, &profile)?;
    for publish in organ_publishers {
        publish(cx, lib_id)?;
    }
    Ok(profile)
}

/// Manifest id used for the load receipt that owns a language profile.
pub fn language_profile_lib_symbol(profile: &Symbol) -> Symbol {
    Symbol::qualified("standard/profile", profile.to_string())
}

#[derive(Clone)]
struct LanguageProfileLib {
    profile: LanguageProfile,
}

impl LanguageProfileLib {
    fn new(profile: LanguageProfile) -> Self {
        Self { profile }
    }
}

impl Lib for LanguageProfileLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: language_profile_lib_symbol(&self.profile.symbol),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Value {
                symbol: self.profile.symbol.clone(),
            }],
        }
    }

    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.value(
            self.profile.symbol.clone(),
            cx.factory()
                .opaque(Arc::new(LanguageProfileValue::new(self.profile.clone())))?,
        )
    }
}

fn ensure_profile_lib(cx: &mut Cx, profile: &LanguageProfile) -> Result<LibId> {
    let lib = LanguageProfileLib::new(profile.clone());
    let manifest = lib.manifest();
    if let Some(loaded) = cx.registry().lib(&manifest.id) {
        return Ok(loaded.id);
    }
    cx.load_lib(&lib)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_symbol() -> Symbol {
        Symbol::qualified("test", "profile")
    }

    fn test_symbol() -> Symbol {
        Symbol::qualified("test", "conformance")
    }

    fn badge_symbol() -> Symbol {
        Symbol::qualified("test", "badge")
    }

    #[test]
    fn fidelity_badge_matches_manual_construction() {
        let profile = profile_symbol();
        let test = test_symbol();
        let helper = fidelity_badge(&profile, badge_symbol(), 1, &test);
        let manual = FidelityBadge::new(
            Ref::Symbol(profile.clone()),
            badge_symbol(),
            1,
            Ref::Symbol(test.clone()),
        );
        assert_eq!(helper, manual);
    }

    #[test]
    fn spec_into_badge_matches_fidelity_badge() {
        let profile = profile_symbol();
        let spec = FidelityBadgeSpec {
            badge: badge_symbol(),
            level: 2,
            test: test_symbol(),
        };
        let from_spec = spec.into_badge(&profile);
        let direct = fidelity_badge(&profile, badge_symbol(), 2, &test_symbol());
        assert_eq!(from_spec, direct);
    }

    #[test]
    fn install_language_profile_registers_and_runs_publishers_in_order() {
        use std::cell::RefCell;
        use std::sync::Arc;

        use sim_kernel::{DefaultFactory, NoopEvalPolicy};

        thread_local! {
            static CALLS: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        }

        fn first(_cx: &mut Cx, _lib_id: LibId) -> Result<()> {
            CALLS.with(|calls| calls.borrow_mut().push(1));
            Ok(())
        }

        fn second(_cx: &mut Cx, _lib_id: LibId) -> Result<()> {
            CALLS.with(|calls| calls.borrow_mut().push(2));
            Ok(())
        }

        CALLS.with(|calls| calls.borrow_mut().clear());

        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let mut registry = ProfileRegistry::new();
        let profile = LanguageProfile::new(profile_symbol());

        let installed =
            install_language_profile(&mut cx, &mut registry, profile.clone(), &[first, second])
                .expect("install language profile");

        assert_eq!(installed, profile);
        assert!(registry.profile(&profile_symbol()).is_some());
        CALLS.with(|calls| assert_eq!(*calls.borrow(), vec![1, 2]));
    }
}
