use std::sync::Arc;

use sim_kernel::{
    Claim, ClaimPattern, Cx, Expr, LibId, Ref, Result, Shape, Symbol, card::card_kind_predicate,
};
use sim_shape::{AnyShape, CaptureShape, ExactExprShape, ListShape};

use crate::{scheme_base_export_kind_symbol, symbols::scheme_symbol};

/// Whether a Scheme surface form is supported by this profile.
#[derive(Clone)]
pub enum SchemeFormStatus {
    /// The form lowers to runtime behavior.
    Supported,
    /// The form is recognized but deferred; the string explains why.
    Unsupported(&'static str),
}

/// Specification of one R7RS-small surface form: its symbol, doc, match shape,
/// and support status.
#[derive(Clone)]
pub struct SchemeFormSpec {
    /// Surface symbol naming the form.
    pub symbol: Symbol,
    /// One-line description of the form.
    pub doc: &'static str,
    /// `Shape` matching the form's head and tail.
    pub shape: Arc<dyn Shape>,
    /// Whether the form is supported or deferred.
    pub status: SchemeFormStatus,
}

/// A Scheme base-library export: a surface symbol and its description.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SchemeBaseExport {
    /// `scheme`-qualified symbol of the export.
    pub symbol: Symbol,
    /// One-line description of the export.
    pub doc: &'static str,
}

/// Returns the R7RS-small surface form specifications and their support status.
pub fn r7rs_small_form_specs() -> Vec<SchemeFormSpec> {
    [
        ("quote", "datum quotation", SchemeFormStatus::Supported),
        ("if", "conditional expression", SchemeFormStatus::Supported),
        ("lambda", "procedure literal", SchemeFormStatus::Supported),
        ("define", "definition form", SchemeFormStatus::Supported),
        ("begin", "sequence expression", SchemeFormStatus::Supported),
        (
            "let",
            "lexical binding via the binding organ",
            SchemeFormStatus::Supported,
        ),
        (
            "let*",
            "sequential binding via the binding organ",
            SchemeFormStatus::Supported,
        ),
        (
            "letrec",
            "recursive binding via the binding organ",
            SchemeFormStatus::Supported,
        ),
        (
            "set!",
            "mutation marker reported as restricted",
            SchemeFormStatus::Unsupported("mutation is deferred"),
        ),
        (
            "call/cc",
            "continuation capture beyond one-shot control",
            SchemeFormStatus::Unsupported("full multishot continuations are deferred"),
        ),
        (
            "dynamic-wind",
            "dynamic wind control hooks",
            SchemeFormStatus::Unsupported("dynamic-wind is deferred"),
        ),
        (
            "eval",
            "read-eval",
            SchemeFormStatus::Unsupported("read-eval is capability gated"),
        ),
    ]
    .into_iter()
    .map(|(name, doc, status)| {
        let symbol = Symbol::new(name);
        SchemeFormSpec {
            symbol: symbol.clone(),
            doc,
            shape: Arc::new(ListShape::with_rest(
                vec![Arc::new(ExactExprShape::new(Expr::Symbol(symbol)))],
                Arc::new(CaptureShape::new(
                    Symbol::new("form-tail"),
                    Arc::new(AnyShape),
                )),
            )),
            status,
        }
    })
    .collect()
}

/// Returns the supported R7RS-small forms as base-library exports.
pub fn r7rs_small_base_exports() -> Vec<SchemeBaseExport> {
    r7rs_small_form_specs()
        .into_iter()
        .filter(|form| matches!(form.status, SchemeFormStatus::Supported))
        .map(|form| SchemeBaseExport {
            symbol: scheme_symbol(&form.symbol.to_string()),
            doc: form.doc,
        })
        .collect()
}

/// Publishes idempotent base-export card claims for the supported forms.
pub fn publish_scheme_base_claims(cx: &mut Cx) -> Result<()> {
    publish_scheme_base_claims_with_owner(cx, None)
}

/// Publishes base-export card claims as part of a loaded lib receipt.
pub fn publish_scheme_base_claims_for_lib(cx: &mut Cx, lib_id: LibId) -> Result<()> {
    publish_scheme_base_claims_with_owner(cx, Some(lib_id))
}

fn publish_scheme_base_claims_with_owner(cx: &mut Cx, owner: Option<LibId>) -> Result<()> {
    for export in r7rs_small_base_exports() {
        insert_once(
            cx,
            owner,
            Ref::Symbol(export.symbol),
            card_kind_predicate(),
            Ref::Symbol(scheme_base_export_kind_symbol()),
        )?;
    }
    Ok(())
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
        let claim = Claim::public(subject, predicate, object);
        match owner {
            Some(lib_id) => {
                cx.insert_fact_for_lib(lib_id, claim)?;
            }
            None => {
                cx.insert_fact(claim)?;
            }
        }
    }
    Ok(())
}
