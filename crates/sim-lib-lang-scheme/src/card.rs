//! Scheme profile Card helpers.

use sim_kernel::{
    Cx, Ref, Result, Symbol, Value, card::card_for_ref_with_fallback,
    standard::standard_profile_kind,
};
use sim_lib_standard_core::{MatrixRunReport, standard_test_capability};

use crate::{r7rs_small_profile_symbol, run_scheme_matrix_row};

/// Builds the Scheme profile Card with browseable conformance fields.
///
/// When the context lacks `standard.test`, the card reports zero counts and
/// `conformance.fidelity = "unscored"` instead of running the harness.
pub fn scheme_language_card(cx: &mut Cx) -> Result<Value> {
    scheme_language_card_with_generated_coverage(cx, Vec::new())
}

/// Builds the Scheme profile Card and appends generated coverage fields.
///
/// The generated coverage fields are caller-supplied so the Scheme profile does
/// not depend on the generative conformance crate. Curated `conformance.*`
/// fields are always produced by the Scheme matrix row.
pub fn scheme_language_card_with_generated_coverage(
    cx: &mut Cx,
    generated_coverage_fields: Vec<(Symbol, Value)>,
) -> Result<Value> {
    let language = scheme_language_symbol();
    let mut entries = vec![
        (
            Symbol::new("profile"),
            cx.factory().symbol(r7rs_small_profile_symbol())?,
        ),
        (
            Symbol::new("language"),
            cx.factory().symbol(language.clone())?,
        ),
    ];
    entries.extend(scheme_conformance_fields(cx, &language)?);
    entries.extend(generated_coverage_fields);
    let fallback = cx.factory().table(entries)?;
    card_for_ref_with_fallback(
        cx,
        Ref::Symbol(r7rs_small_profile_symbol()),
        Some(fallback),
        Some(standard_profile_kind()),
    )
}

fn scheme_conformance_fields(cx: &mut Cx, language: &Symbol) -> Result<Vec<(Symbol, Value)>> {
    if cx.require(&standard_test_capability()).is_ok() {
        return run_scheme_matrix_row(cx)?.conformance_card_fields(cx, language);
    }
    MatrixRunReport::unscored_conformance_card_fields(cx)
}

fn scheme_language_symbol() -> Symbol {
    Symbol::new("scheme")
}
