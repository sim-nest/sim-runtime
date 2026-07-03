use sim_codec::{DecodeLimits, Input, ReadCx};
use sim_kernel::{
    CapabilitySet, Cx, Diagnostic, Error, ReadPolicy, Ref, Result, Severity, Symbol, TrustLevel,
};
use sim_lib_standard_core::{
    FidelityBadge, LanguageProfile, OrganUse, ProfileRegistry, install_language_profile,
};

use crate::{
    SchemeLowered, decode_scheme_tree, lower_scheme_tree, publish_scheme_base_claims_for_lib,
    r7rs_small_form_specs, r7rs_small_profile_symbol, scheme_conformance_test_symbol,
    scheme_lowering_symbol, scheme_reader_symbol,
};

/// Builds the R7RS-small [`LanguageProfile`]: reader, lowering, eval policy,
/// organ uses, numeric tower, conformance test, fidelity badge, and the
/// unsupported-form set.
///
/// The fidelity badge marks the profile as `standard/partial`: some R7RS-small
/// forms (see [`r7rs_small_form_specs`](crate::r7rs_small_form_specs)) are
/// deferred. See the crate [README] for the language-profile role.
///
/// [README]: https://docs.rs/crate/sim-lib-lang-scheme
pub fn r7rs_small_profile() -> LanguageProfile {
    let profile = r7rs_small_profile_symbol();
    let test = scheme_conformance_test_symbol();
    let mut out = LanguageProfile::new(profile.clone())
        .with_reader(scheme_reader_symbol())
        .with_lowering(scheme_lowering_symbol())
        .with_eval_policy(Symbol::qualified("eval", "restricted"))
        .with_organ(OrganUse::new(sim_lib_control::control_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_binding::binding_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_sequence::sequence_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_pattern::pattern_organ_symbol()))
        .with_numeric_tower(Symbol::qualified("numbers", "scheme-small"))
        .with_conformance_test(test.clone())
        .with_fidelity_badge(FidelityBadge::new(
            Ref::Symbol(profile),
            Symbol::qualified("standard", "partial"),
            1,
            Ref::Symbol(test),
        ));
    for form in r7rs_small_form_specs() {
        if matches!(form.status, crate::SchemeFormStatus::Unsupported(_)) {
            out = out.with_unsupported_form(form.symbol);
        }
    }
    out
}

/// Installs the R7RS-small profile into a [`ProfileRegistry`], publishing its
/// base-export card claims.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
/// use sim_lib_standard_core::ProfileRegistry;
/// use sim_lib_lang_scheme::install_r7rs_small_profile;
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// let mut registry = ProfileRegistry::new();
/// let profile = install_r7rs_small_profile(&mut cx, &mut registry).unwrap();
/// assert!(!profile.unsupported_forms.is_empty());
/// ```
pub fn install_r7rs_small_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
) -> Result<LanguageProfile> {
    install_language_profile(
        cx,
        registry,
        r7rs_small_profile(),
        &[publish_scheme_base_claims_for_lib],
    )
}

/// Collects diagnostics for any deferred R7RS-small forms reachable in `expr`.
pub fn diagnose_unsupported_forms(expr: &sim_kernel::Expr) -> Vec<Diagnostic> {
    let unsupported = r7rs_small_profile()
        .unsupported_forms
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let mut diagnostics = Vec::new();
    collect_unsupported(expr, &unsupported, &mut diagnostics);
    diagnostics
}

/// Decodes and lowers Scheme `source` under untrusted, restricted capabilities.
///
/// Rejects any deferred form (e.g. `eval`) with an error before lowering.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
/// use sim_lib_lang_scheme::run_r7rs_small_restricted;
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// assert!(run_r7rs_small_restricted(&mut cx, "(begin #t #f)").is_ok());
/// assert!(run_r7rs_small_restricted(&mut cx, "(eval '(+ 1 2))").is_err());
/// ```
pub fn run_r7rs_small_restricted(cx: &mut Cx, source: &str) -> Result<SchemeLowered> {
    let read_policy = ReadPolicy {
        trust: TrustLevel::Untrusted,
        capabilities: CapabilitySet::new(),
    };
    let mut read_cx = ReadCx {
        cx,
        codec: sim_kernel::CodecId(0),
        read_policy,
        limits: DecodeLimits::default(),
    };
    let tree = decode_scheme_tree(
        &mut read_cx,
        "restricted-scheme",
        Input::Text(source.to_owned()),
    )?;
    let diagnostics = diagnose_unsupported_forms(&tree.expr);
    if let Some(diagnostic) = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.severity == Severity::Error)
    {
        return Err(Error::Eval(diagnostic.message.clone()));
    }
    lower_scheme_tree(&tree).map(|lowering| lowering.lowered)
}

fn collect_unsupported(
    expr: &sim_kernel::Expr,
    unsupported: &std::collections::BTreeSet<Symbol>,
    out: &mut Vec<Diagnostic>,
) {
    match expr {
        sim_kernel::Expr::List(items) => {
            if let Some(sim_kernel::Expr::Symbol(head)) = items.first()
                && unsupported.contains(head)
            {
                let mut diagnostic =
                    Diagnostic::error(format!("unsupported R7RS-small form {head}"));
                diagnostic.code = Some(Symbol::qualified("scheme", "unsupported-form"));
                out.push(diagnostic);
            }
            for item in items {
                collect_unsupported(item, unsupported, out);
            }
        }
        sim_kernel::Expr::Call { operator, args } => {
            collect_unsupported(operator, unsupported, out);
            for arg in args {
                collect_unsupported(arg, unsupported, out);
            }
        }
        sim_kernel::Expr::Block(items)
        | sim_kernel::Expr::Vector(items)
        | sim_kernel::Expr::Set(items) => {
            for item in items {
                collect_unsupported(item, unsupported, out);
            }
        }
        sim_kernel::Expr::Map(entries) => {
            for (key, value) in entries {
                collect_unsupported(key, unsupported, out);
                collect_unsupported(value, unsupported, out);
            }
        }
        sim_kernel::Expr::Quote { expr, .. }
        | sim_kernel::Expr::Annotated { expr, .. }
        | sim_kernel::Expr::Extension { payload: expr, .. }
        | sim_kernel::Expr::Prefix { arg: expr, .. }
        | sim_kernel::Expr::Postfix { arg: expr, .. } => {
            collect_unsupported(expr, unsupported, out)
        }
        sim_kernel::Expr::Infix { left, right, .. } => {
            collect_unsupported(left, unsupported, out);
            collect_unsupported(right, unsupported, out);
        }
        sim_kernel::Expr::Nil
        | sim_kernel::Expr::Bool(_)
        | sim_kernel::Expr::Number(_)
        | sim_kernel::Expr::Symbol(_)
        | sim_kernel::Expr::Local(_)
        | sim_kernel::Expr::String(_)
        | sim_kernel::Expr::Bytes(_) => {}
    }
}
