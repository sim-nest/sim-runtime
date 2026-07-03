use sim_kernel::{Ref, Symbol};
use sim_lib_pattern::{MatchArm, match_value};
use sim_lib_standard_core::ProfileRegistry;

use crate::*;

use sim_kernel::testing::bare_cx as cx;

#[test]
fn typed_lazy_adts_use_pattern_organ() {
    let mut cx = cx();
    let option = typed_lazy_option_type();
    let none = option
        .constructor(&Symbol::qualified("typed-lazy/option", "None"))
        .unwrap();
    let some = option
        .constructor(&Symbol::qualified("typed-lazy/option", "Some"))
        .unwrap();
    let payload = cx
        .factory()
        .symbol(Symbol::qualified("value", "payload"))
        .unwrap();
    let value = some.construct(&mut cx, vec![payload]).unwrap();
    let matched = match_value(
        &mut cx,
        value,
        &[
            MatchArm::for_constructor(&none),
            MatchArm::for_constructor(&some),
        ],
    )
    .unwrap();

    assert_eq!(
        matched.label(),
        &Symbol::qualified("typed-lazy/option", "Some")
    );
    assert!(
        typed_lazy_profile()
            .organs
            .iter()
            .any(|organ| organ.organ == sim_lib_pattern::pattern_organ_symbol())
    );
}

#[test]
fn typed_lazy_dictionaries_and_lazy_refs_are_explicitly_limited() {
    let dict = TypeclassDictionary::new(
        Symbol::qualified("class", "Show"),
        Symbol::qualified("type", "Option"),
    )
    .add_method(
        Symbol::new("show"),
        Symbol::qualified("typed-lazy", "show-option"),
    );
    assert_eq!(
        dict.method(&Symbol::new("show")),
        Some(&Symbol::qualified("typed-lazy", "show-option"))
    );

    let mut lazy = LazyRef::new(Ref::Symbol(Symbol::qualified("lazy", "value")));
    assert!(!lazy.is_forced());
    assert_eq!(
        lazy.force(),
        Ref::Symbol(Symbol::qualified("lazy", "value"))
    );
    assert_eq!(
        lazy.force(),
        Ref::Symbol(Symbol::qualified("lazy", "value"))
    );
    assert!(lazy.is_forced());
    assert!(
        typed_lazy_profile()
            .fidelity_badges
            .iter()
            .any(|badge| badge.badge == typed_lazy_typeclass_fidelity_symbol() && badge.level == 0)
    );
    assert!(
        typed_lazy_profile()
            .organs
            .iter()
            .any(|organ| organ.organ == sim_lib_control::control_organ_symbol())
    );
}

#[test]
fn typed_lazy_profile_publishes_pattern_claims() {
    let mut cx = cx();
    let mut registry = ProfileRegistry::new();
    let profile = install_typed_lazy_profile(&mut cx, &mut registry).unwrap();

    assert!(
        profile
            .fidelity_badges
            .iter()
            .any(|badge| badge.badge == typed_lazy_pattern_fidelity_symbol() && badge.level == 1)
    );
    assert!(registry.profile(&profile.symbol).is_some());
    assert_eq!(
        cx.query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(sim_lib_pattern::pattern_organ_symbol())),
            predicate: Some(sim_kernel::card::card_kind_predicate()),
            object: Some(Ref::Symbol(sim_kernel::standard::standard_organ_kind())),
            include_revoked: false,
        })
        .unwrap()
        .len(),
        1
    );
}
