//! Claim publication helpers for conformance matrix cells.

use sim_kernel::{
    Claim, ClaimKind, ClaimPattern, Cx, Datum, DatumStore, Ref, Result, Symbol,
    card::{card_kind_predicate, card_tests_predicate},
    standard::standard_evidence_predicate,
};

use crate::{
    MatrixCellResult, standard_test_case_predicate, standard_test_organ_predicate,
    standard_test_profile_predicate, standard_test_result_predicate, standard_test_run_kind,
    standard_test_status_predicate,
};

pub(crate) fn publish_matrix_cell_claim(cx: &mut Cx, cell: &MatrixCellResult) -> Result<()> {
    let evidence = matrix_cell_ref(cx, cell)?;
    insert_observed_once(
        cx,
        evidence.clone(),
        card_kind_predicate(),
        Ref::Symbol(standard_test_run_kind()),
    )?;
    insert_observed_once(
        cx,
        evidence.clone(),
        card_tests_predicate(),
        Ref::Symbol(cell.case_symbol.clone()),
    )?;
    insert_observed_once(
        cx,
        evidence.clone(),
        standard_test_profile_predicate(),
        Ref::Symbol(cell.profile.clone()),
    )?;
    insert_observed_once(
        cx,
        evidence.clone(),
        standard_test_organ_predicate(),
        Ref::Symbol(cell.organ.clone()),
    )?;
    insert_observed_once(
        cx,
        evidence.clone(),
        standard_test_case_predicate(),
        Ref::Symbol(cell.case_symbol.clone()),
    )?;
    insert_observed_once(
        cx,
        evidence.clone(),
        standard_test_status_predicate(),
        Ref::Symbol(cell.outcome.status_symbol()),
    )?;
    insert_observed_once(
        cx,
        Ref::Symbol(cell.profile.clone()),
        standard_test_result_predicate(),
        evidence.clone(),
    )?;
    insert_observed_once(
        cx,
        Ref::Symbol(cell.organ.clone()),
        standard_test_result_predicate(),
        evidence.clone(),
    )?;
    insert_observed_once(
        cx,
        Ref::Symbol(cell.profile.clone()),
        standard_evidence_predicate(),
        evidence,
    )
}

fn matrix_cell_ref(cx: &mut Cx, cell: &MatrixCellResult) -> Result<Ref> {
    let mut fields = vec![
        (Symbol::new("profile"), Datum::Symbol(cell.profile.clone())),
        (
            Symbol::new("language"),
            Datum::Symbol(cell.language.clone()),
        ),
        (Symbol::new("organ"), Datum::Symbol(cell.organ.clone())),
        (Symbol::new("test"), Datum::Symbol(cell.case_symbol.clone())),
        (Symbol::new("passed"), Datum::Bool(cell.outcome.passed)),
        (
            Symbol::new("status"),
            Datum::Symbol(cell.outcome.status_symbol()),
        ),
    ];
    if let Some(detail) = &cell.outcome.detail {
        fields.push((Symbol::new("detail"), Datum::String(detail.clone())));
    }
    cx.datum_store_mut()
        .intern(Datum::Node {
            tag: standard_test_run_kind(),
            fields,
        })
        .map(Ref::Content)
}

fn insert_observed_once(cx: &mut Cx, subject: Ref, predicate: Symbol, object: Ref) -> Result<()> {
    let exists = !cx
        .query_facts(ClaimPattern::exact(
            subject.clone(),
            predicate.clone(),
            object.clone(),
        ))?
        .is_empty();
    if !exists {
        cx.insert_fact(Claim::public(subject, predicate, object).with_kind(ClaimKind::Observed))?;
    }
    Ok(())
}
