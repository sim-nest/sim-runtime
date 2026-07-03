//! Standard-distribution claim predicates and profile/badge claim publishing.

use sim_kernel::{
    Claim, ClaimKind, ClaimPattern, Cx, LibId, Ref, Result, Symbol, card::card_requires_predicate,
};

use crate::{FidelityBadge, LanguageProfile};

/// Claim predicate relating a profile to its reader symbol.
pub fn standard_reader_predicate() -> Symbol {
    standard_symbol("reader")
}

/// Claim predicate relating a profile to its lowering symbol.
pub fn standard_lowering_predicate() -> Symbol {
    standard_symbol("lowering")
}

/// Claim predicate relating a profile to its eval-policy symbol.
pub fn standard_eval_policy_predicate() -> Symbol {
    standard_symbol("eval-policy")
}

/// Claim predicate relating a profile to its numeric-tower symbol.
pub fn standard_numeric_predicate() -> Symbol {
    standard_symbol("numeric")
}

/// Claim predicate relating a profile to a capability it requires.
pub fn standard_capability_predicate() -> Symbol {
    standard_symbol("capability")
}

/// Claim predicate relating a profile to a form it does not support.
pub fn standard_unsupported_predicate() -> Symbol {
    standard_symbol("unsupported")
}

/// Publish the full claim set for `profile`: reader, lowering, eval-policy,
/// numeric tower, capabilities, unsupported forms, and fidelity badges.
///
/// Each fact is inserted at most once, so republishing an unchanged profile is a
/// no-op. The claim store and predicate contracts are defined by the kernel; see
/// the crate README for the contract sections.
pub fn publish_profile_claims(cx: &mut Cx, profile: &LanguageProfile) -> Result<()> {
    publish_profile_claims_with_owner(cx, None, profile)
}

/// Publish profile claims as part of a loaded lib receipt.
pub fn publish_profile_claims_for_lib(
    cx: &mut Cx,
    lib_id: LibId,
    profile: &LanguageProfile,
) -> Result<()> {
    publish_profile_claims_with_owner(cx, Some(lib_id), profile)
}

fn publish_profile_claims_with_owner(
    cx: &mut Cx,
    owner: Option<LibId>,
    profile: &LanguageProfile,
) -> Result<()> {
    match owner {
        Some(lib_id) => sim_kernel::standard::publish_profile_claims_for_lib(
            cx,
            lib_id,
            profile.symbol.clone(),
            profile.organs.iter().map(|organ| organ.organ.clone()),
            profile.conformance_tests.iter().cloned(),
        )?,
        None => sim_kernel::standard::publish_profile_claims(
            cx,
            profile.symbol.clone(),
            profile.organs.iter().map(|organ| organ.organ.clone()),
            profile.conformance_tests.iter().cloned(),
        )?,
    }

    let subject = Ref::Symbol(profile.symbol.clone());
    insert_once(
        cx,
        owner,
        subject.clone(),
        standard_reader_predicate(),
        Ref::Symbol(profile.reader.clone()),
    )?;
    insert_once(
        cx,
        owner,
        subject.clone(),
        standard_lowering_predicate(),
        Ref::Symbol(profile.lowering.clone()),
    )?;
    insert_once(
        cx,
        owner,
        subject.clone(),
        standard_eval_policy_predicate(),
        Ref::Symbol(profile.eval_policy.clone()),
    )?;
    if let Some(numeric_tower) = &profile.numeric_tower {
        insert_once(
            cx,
            owner,
            subject.clone(),
            standard_numeric_predicate(),
            Ref::Symbol(numeric_tower.clone()),
        )?;
    }
    for capability in &profile.capabilities {
        insert_once(
            cx,
            owner,
            subject.clone(),
            standard_capability_predicate(),
            Ref::Symbol(capability.as_symbol()),
        )?;
        insert_once(
            cx,
            owner,
            subject.clone(),
            card_requires_predicate(),
            Ref::Symbol(capability.as_symbol()),
        )?;
    }
    for form in &profile.unsupported_forms {
        insert_once(
            cx,
            owner,
            subject.clone(),
            standard_unsupported_predicate(),
            Ref::Symbol(form.clone()),
        )?;
    }
    for badge in &profile.fidelity_badges {
        publish_badge_claims_with_owner(cx, owner, badge)?;
    }
    Ok(())
}

/// Publish the claim set for a single [`FidelityBadge`]: its badge fact,
/// fidelity level, and the observed badge claim, each inserted at most once.
pub fn publish_badge_claims(cx: &mut Cx, badge: &FidelityBadge) -> Result<()> {
    publish_badge_claims_with_owner(cx, None, badge)
}

/// Publish badge claims as part of a loaded lib receipt.
pub fn publish_badge_claims_for_lib(
    cx: &mut Cx,
    lib_id: LibId,
    badge: &FidelityBadge,
) -> Result<()> {
    publish_badge_claims_with_owner(cx, Some(lib_id), badge)
}

fn publish_badge_claims_with_owner(
    cx: &mut Cx,
    owner: Option<LibId>,
    badge: &FidelityBadge,
) -> Result<()> {
    match owner {
        Some(lib_id) => sim_kernel::standard::publish_fidelity_badge_for_lib(
            cx,
            lib_id,
            badge.subject.clone(),
            badge.badge.clone(),
            badge.evidence.clone(),
        )?,
        None => sim_kernel::standard::publish_fidelity_badge(
            cx,
            badge.subject.clone(),
            badge.badge.clone(),
            badge.evidence.clone(),
        )?,
    }
    insert_once(
        cx,
        owner,
        badge.subject.clone(),
        standard_symbol("fidelity-level"),
        Ref::Symbol(Symbol::qualified(
            "standard/fidelity-level",
            badge.level.to_string(),
        )),
    )?;
    insert_observed_once(
        cx,
        owner,
        badge.subject.clone(),
        standard_symbol("fidelity-badge"),
        Ref::Symbol(badge.badge.clone()),
    )
}

fn insert_once(
    cx: &mut Cx,
    owner: Option<LibId>,
    subject: Ref,
    predicate: Symbol,
    object: Ref,
) -> Result<()> {
    let exists = !cx
        .query_facts(ClaimPattern::exact(
            subject.clone(),
            predicate.clone(),
            object.clone(),
        ))?
        .is_empty();
    if !exists {
        insert_claim(cx, owner, Claim::public(subject, predicate, object))?;
    }
    Ok(())
}

fn insert_observed_once(
    cx: &mut Cx,
    owner: Option<LibId>,
    subject: Ref,
    predicate: Symbol,
    object: Ref,
) -> Result<()> {
    let exists = !cx
        .query_facts(ClaimPattern::exact(
            subject.clone(),
            predicate.clone(),
            object.clone(),
        ))?
        .is_empty();
    if !exists {
        insert_claim(
            cx,
            owner,
            Claim::public(subject, predicate, object).with_kind(ClaimKind::Observed),
        )?;
    }
    Ok(())
}

fn insert_claim(cx: &mut Cx, owner: Option<LibId>, claim: Claim) -> Result<()> {
    match owner {
        Some(lib_id) => {
            cx.insert_fact_for_lib(lib_id, claim)?;
        }
        None => {
            cx.insert_fact(claim)?;
        }
    }
    Ok(())
}

fn standard_symbol(name: &str) -> Symbol {
    Symbol::qualified("standard", name.to_owned())
}
