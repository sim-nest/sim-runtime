//! Profile diffing: structural comparison of two language profiles.

use sim_kernel::{Cx, Expr, OpKey, Result, Symbol};

use crate::{LanguageProfile, standard_diff_capability};

/// Whether two profiles compared equal or differed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileDiffStatus {
    /// The two profiles are structurally identical.
    Same,
    /// The two profiles differ in at least one field.
    Different,
}

/// A single differing field between two profiles, with both sides as
/// expressions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileDifference {
    /// Qualified name of the differing field.
    pub field: Symbol,
    /// Left-side value.
    pub left: Expr,
    /// Right-side value.
    pub right: Expr,
}

/// Structural comparison of two language profiles, produced by
/// [`standard_diff_stub`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileDiff {
    /// Symbol of the left profile.
    pub left: Symbol,
    /// Symbol of the right profile.
    pub right: Symbol,
    /// Overall same/different status.
    pub status: ProfileDiffStatus,
    /// Organs used by both profiles.
    pub shared_organs: Vec<Symbol>,
    /// Organs used only by the left profile.
    pub left_only_organs: Vec<Symbol>,
    /// Organs used only by the right profile.
    pub right_only_organs: Vec<Symbol>,
    /// Per-field differences.
    pub differences: Vec<ProfileDifference>,
}

impl ProfileDiff {
    /// Whether the two profiles compared as identical.
    pub fn is_same(&self) -> bool {
        self.status == ProfileDiffStatus::Same
    }
}

/// Operation key for the standard diff operation.
pub fn standard_diff_op_key() -> OpKey {
    OpKey::new(Symbol::new("standard"), Symbol::new("diff"), 1)
}

/// Symbol naming the profile-diff operation on the codec surface.
pub fn profile_diff_symbol() -> Symbol {
    Symbol::qualified("profile", "diff")
}

/// Diff `left` against `right`, gated on [`standard_diff_capability`].
///
/// Compares reader, lowering, eval-policy, organs, numeric tower, capabilities,
/// unsupported forms, and conformance tests, returning a [`ProfileDiff`].
///
/// [`standard_diff_capability`]: crate::standard_diff_capability
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
///
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, Symbol};
/// use sim_lib_standard_core::{LanguageProfile, standard_diff_capability, standard_diff_stub};
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// cx.grant(standard_diff_capability());
///
/// let left = LanguageProfile::new(Symbol::qualified("lang", "a/v1"))
///     .with_reader(Symbol::qualified("codec", "lisp"));
/// let right = LanguageProfile::new(Symbol::qualified("lang", "b/v1"))
///     .with_reader(Symbol::qualified("codec", "json"));
///
/// assert!(standard_diff_stub(&cx, &left, &left).unwrap().is_same());
/// assert!(!standard_diff_stub(&cx, &left, &right).unwrap().is_same());
/// ```
pub fn standard_diff_stub(
    cx: &Cx,
    left: &LanguageProfile,
    right: &LanguageProfile,
) -> Result<ProfileDiff> {
    cx.require(&standard_diff_capability())?;
    let left_organs = left
        .organs
        .iter()
        .map(|organ| organ.organ.clone())
        .collect::<Vec<_>>();
    let right_organs = right
        .organs
        .iter()
        .map(|organ| organ.organ.clone())
        .collect::<Vec<_>>();
    let mut differences = Vec::new();
    push_difference(
        &mut differences,
        "reader",
        Expr::Symbol(left.reader.clone()),
        Expr::Symbol(right.reader.clone()),
    );
    push_difference(
        &mut differences,
        "lowering",
        Expr::Symbol(left.lowering.clone()),
        Expr::Symbol(right.lowering.clone()),
    );
    push_difference(
        &mut differences,
        "eval-policy",
        Expr::Symbol(left.eval_policy.clone()),
        Expr::Symbol(right.eval_policy.clone()),
    );
    push_difference(
        &mut differences,
        "organs",
        symbols_expr(&left_organs),
        symbols_expr(&right_organs),
    );
    push_difference(
        &mut differences,
        "numeric",
        optional_symbol_expr(left.numeric_tower.as_ref()),
        optional_symbol_expr(right.numeric_tower.as_ref()),
    );
    push_difference(
        &mut differences,
        "capabilities",
        capability_expr(left),
        capability_expr(right),
    );
    push_difference(
        &mut differences,
        "unsupported",
        symbols_expr(&left.unsupported_forms),
        symbols_expr(&right.unsupported_forms),
    );
    push_difference(
        &mut differences,
        "conformance-tests",
        symbols_expr(&left.conformance_tests),
        symbols_expr(&right.conformance_tests),
    );

    Ok(ProfileDiff {
        left: left.symbol.clone(),
        right: right.symbol.clone(),
        status: if differences.is_empty() {
            ProfileDiffStatus::Same
        } else {
            ProfileDiffStatus::Different
        },
        shared_organs: left_organs
            .iter()
            .filter(|organ| right_organs.contains(organ))
            .cloned()
            .collect(),
        left_only_organs: left_organs
            .iter()
            .filter(|organ| !right_organs.contains(organ))
            .cloned()
            .collect(),
        right_only_organs: right_organs
            .iter()
            .filter(|organ| !left_organs.contains(organ))
            .cloned()
            .collect(),
        differences,
    })
}

fn push_difference(differences: &mut Vec<ProfileDifference>, field: &str, left: Expr, right: Expr) {
    if left != right {
        differences.push(ProfileDifference {
            field: Symbol::qualified("profile/diff", field.to_owned()),
            left,
            right,
        });
    }
}

fn symbols_expr(symbols: &[Symbol]) -> Expr {
    Expr::List(symbols.iter().cloned().map(Expr::Symbol).collect())
}

fn optional_symbol_expr(symbol: Option<&Symbol>) -> Expr {
    symbol.cloned().map(Expr::Symbol).unwrap_or(Expr::Nil)
}

fn capability_expr(profile: &LanguageProfile) -> Expr {
    Expr::List(
        profile
            .capabilities
            .iter()
            .map(|capability| Expr::Symbol(capability.as_symbol()))
            .collect(),
    )
}
