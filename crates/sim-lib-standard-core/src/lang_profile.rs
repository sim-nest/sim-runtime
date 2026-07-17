//! Shared language-profile helpers.
//!
//! Every per-language crate (`sim-lib-lang-*`) builds a [`LanguageProfile`],
//! attaches [`FidelityBadge`]s to it, and installs it with the same fixed
//! sequence: resolve backing libraries, register the profile, publish the live
//! profile claims, then publish any extra metadata claims owned by the profile
//! receipt. These helpers capture that shared shape so the per-crate code only
//! carries the language-specific definitions.

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

type ProfileBackingInstaller = fn(&mut Cx) -> Result<()>;

/// One backing library requirement for a profile organ.
#[derive(Clone)]
pub struct ProfileBackingLib {
    organ: Symbol,
    manifest: Symbol,
    install: Option<ProfileBackingInstaller>,
    publish_claims: Option<ProfileOrganPublisher>,
}

impl ProfileBackingLib {
    /// Declare a loadable backing library for `organ`.
    pub fn loadable(
        organ: Symbol,
        manifest: Symbol,
        install: ProfileBackingInstaller,
        publish_claims: Option<ProfileOrganPublisher>,
    ) -> Self {
        Self {
            organ,
            manifest,
            install: Some(install),
            publish_claims,
        }
    }

    /// Declare a backing library requirement for `organ` that is not yet
    /// loadable in the current runtime.
    pub fn unresolved(organ: Symbol, manifest: Symbol) -> Self {
        Self {
            organ,
            manifest,
            install: None,
            publish_claims: None,
        }
    }
}

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

/// Install a fully-built [`LanguageProfile`]: resolve its backing libraries,
/// register the resulting live profile, publish its profile claims, then run
/// any extra metadata publishers against the profile receipt.
///
/// Organs whose backing libraries are not yet loadable are removed from the
/// live claim set and recorded under
/// [`LanguageProfile::backing_requirements`](crate::LanguageProfile::backing_requirements)
/// instead.
pub fn install_language_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
    profile: LanguageProfile,
    backing_libs: &[ProfileBackingLib],
    extra_publishers: &[ProfileOrganPublisher],
) -> Result<LanguageProfile> {
    let profile = resolve_profile_backings(cx, profile, backing_libs)?;
    let lib_id = ensure_profile_lib(cx, &profile)?;
    registry.register_profile(profile.clone())?;
    publish_profile_claims_for_lib(cx, lib_id, &profile)?;
    for publish in extra_publishers {
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

fn resolve_profile_backings(
    cx: &mut Cx,
    profile: LanguageProfile,
    backing_libs: &[ProfileBackingLib],
) -> Result<LanguageProfile> {
    let mut declared = std::collections::BTreeMap::new();
    for backing in backing_libs {
        let replaced = declared.insert(backing.organ.clone(), backing.clone());
        if replaced.is_some() {
            return Err(sim_kernel::Error::Eval(format!(
                "duplicate backing library spec for organ {}",
                backing.organ
            )));
        }
    }

    let requested_organs = profile.organs.clone();
    let mut resolved = profile;
    resolved.organs.clear();
    resolved.backing_requirements.clear();

    for organ in requested_organs {
        let backing = declared.remove(&organ.organ).unwrap_or_else(|| {
            ProfileBackingLib::unresolved(
                organ.organ.clone(),
                default_backing_manifest_for_organ(&organ.organ),
            )
        });
        if let Some(loaded) = resolve_loaded_backing(cx, &backing)? {
            resolved.organs.push(organ);
            if let Some(publish_claims) = backing.publish_claims {
                publish_claims(cx, loaded.id)?;
            }
        } else {
            resolved = resolved.with_backing_requirement(backing.manifest);
        }
    }

    if let Some(unused) = declared.into_values().next() {
        return Err(sim_kernel::Error::Eval(format!(
            "backing library spec for {} does not match any declared organ",
            unused.organ
        )));
    }

    Ok(resolved)
}

fn resolve_loaded_backing(
    cx: &mut Cx,
    backing: &ProfileBackingLib,
) -> Result<Option<sim_kernel::LoadedLib>> {
    let Some(install) = backing.install else {
        return Ok(None);
    };
    install(cx)?;
    let loaded = cx
        .registry()
        .lib(&backing.manifest)
        .cloned()
        .ok_or_else(|| {
            sim_kernel::Error::Eval(format!(
                "backing library {} for organ {} did not load",
                backing.manifest, backing.organ
            ))
        })?;
    if loaded.exports.is_empty() {
        return Err(sim_kernel::Error::Eval(format!(
            "backing library {} for organ {} published no live exports",
            backing.manifest, backing.organ
        )));
    }
    Ok(Some(loaded))
}

fn default_backing_manifest_for_organ(organ: &Symbol) -> Symbol {
    Symbol::qualified("sim", organ.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

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
        use std::sync::Arc;

        use sim_kernel::{
            AbiVersion, DefaultFactory, Export, Lib, LibManifest, LibTarget, LoadCx,
            NoopEvalPolicy, Version,
        };

        thread_local! {
            static CALLS: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
        }

        struct FixtureLib {
            manifest: Symbol,
            export: Symbol,
        }

        impl Lib for FixtureLib {
            fn manifest(&self) -> LibManifest {
                LibManifest {
                    id: self.manifest.clone(),
                    version: Version("0.1.0".to_owned()),
                    abi: AbiVersion { major: 0, minor: 1 },
                    target: LibTarget::HostRegistered,
                    requires: Vec::new(),
                    capabilities: Vec::new(),
                    exports: vec![Export::Value {
                        symbol: self.export.clone(),
                    }],
                }
            }

            fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
                linker.value(self.export.clone(), cx.factory().bool(true)?)?;
                Ok(())
            }
        }

        fn install_backing_one(cx: &mut Cx) -> Result<()> {
            let manifest = Symbol::qualified("sim", "organ-one");
            if cx.registry().lib(&manifest).is_none() {
                cx.load_lib(&FixtureLib {
                    manifest: manifest.clone(),
                    export: Symbol::qualified("test", "backing-one"),
                })?;
            }
            Ok(())
        }

        fn install_backing_two(cx: &mut Cx) -> Result<()> {
            let manifest = Symbol::qualified("sim", "organ-two");
            if cx.registry().lib(&manifest).is_none() {
                cx.load_lib(&FixtureLib {
                    manifest: manifest.clone(),
                    export: Symbol::qualified("test", "backing-two"),
                })?;
            }
            Ok(())
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
        let profile = LanguageProfile::new(profile_symbol())
            .with_organ(crate::OrganUse::new(Symbol::qualified("organ", "one")))
            .with_organ(crate::OrganUse::new(Symbol::qualified("organ", "two")));

        let installed = install_language_profile(
            &mut cx,
            &mut registry,
            profile.clone(),
            &[
                ProfileBackingLib::loadable(
                    Symbol::qualified("organ", "one"),
                    Symbol::qualified("sim", "organ-one"),
                    install_backing_one,
                    Some(first),
                ),
                ProfileBackingLib::loadable(
                    Symbol::qualified("organ", "two"),
                    Symbol::qualified("sim", "organ-two"),
                    install_backing_two,
                    Some(second),
                ),
            ],
            &[],
        )
        .expect("install language profile");

        assert_eq!(installed, profile);
        assert!(registry.profile(&profile_symbol()).is_some());
        CALLS.with(|calls| assert_eq!(*calls.borrow(), vec![1, 2]));
    }

    #[test]
    fn install_language_profile_records_unresolved_backing_requirements() {
        use std::sync::Arc;

        use sim_kernel::{
            ClaimPattern, DefaultFactory, NoopEvalPolicy, Ref, card::card_kind_predicate,
            standard::standard_organ_predicate, standard::standard_profile_kind,
        };

        let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let mut registry = ProfileRegistry::new();
        let organ = Symbol::qualified("organ", "missing");
        let profile =
            LanguageProfile::new(profile_symbol()).with_organ(crate::OrganUse::new(organ.clone()));

        let installed = install_language_profile(&mut cx, &mut registry, profile, &[], &[])
            .expect("install language profile");

        assert!(installed.organs.is_empty());
        assert_eq!(
            installed.backing_requirements,
            vec![Symbol::qualified("sim", "missing")]
        );
        assert_eq!(
            registry
                .profile(&profile_symbol())
                .unwrap()
                .backing_requirements,
            vec![Symbol::qualified("sim", "missing")]
        );
        assert_eq!(
            cx.query_facts(ClaimPattern::exact(
                Ref::Symbol(profile_symbol()),
                card_kind_predicate(),
                Ref::Symbol(standard_profile_kind()),
            ))
            .unwrap()
            .len(),
            1
        );
        assert!(
            cx.query_facts(ClaimPattern::exact(
                Ref::Symbol(profile_symbol()),
                standard_organ_predicate(),
                Ref::Symbol(organ),
            ))
            .unwrap()
            .is_empty()
        );
    }
}
