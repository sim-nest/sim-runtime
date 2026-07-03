use std::sync::Arc;

use sim_kernel::{
    Expr, Ref, Symbol,
    card::{card_for_ref, card_kind_predicate},
    force_list_to_vec,
    standard::standard_organ_kind,
};
use sim_shape::{AnyShape, CaptureShape, ExprKindShape};

use crate::*;

use sim_kernel::testing::bare_cx as cx;

fn maybe_type() -> AlgebraicDataType {
    AlgebraicDataType::new(
        Symbol::qualified("adt", "Maybe"),
        vec![
            VariantDeclaration::nullary(Symbol::qualified("maybe", "Nothing")),
            VariantDeclaration::new(
                Symbol::qualified("maybe", "Just"),
                vec![PatternField::new(
                    Symbol::new("value"),
                    Arc::new(CaptureShape::new(
                        Symbol::new("payload"),
                        Arc::new(AnyShape),
                    )),
                )],
            ),
        ],
    )
    .unwrap()
}

#[test]
fn maybe_style_declaration_builds_constructors() {
    let mut cx = cx();
    let maybe = maybe_type();
    let constructors = maybe.constructors();
    assert_eq!(constructors.len(), 2);

    let just = maybe
        .constructor(&Symbol::qualified("maybe", "Just"))
        .expect("Just constructor");
    let constructor_value = just.as_value(&mut cx).unwrap();
    assert!(constructor_value.object().as_callable().is_some());

    let payload = cx.factory().string("ok".to_owned()).unwrap();
    let tagged = just.construct(&mut cx, vec![payload]).unwrap();
    let tagged = tagged_value(&tagged).expect("tagged value");
    assert_eq!(tagged.adt(), maybe.symbol());
    assert_eq!(tagged.variant(), &Symbol::qualified("maybe", "Just"));
    assert!(tagged.field(&Symbol::new("value")).is_some());
}

#[test]
fn match_selects_correct_arm_and_binds_captures() {
    let mut cx = cx();
    let maybe = maybe_type();
    let nothing = maybe
        .constructor(&Symbol::qualified("maybe", "Nothing"))
        .expect("Nothing constructor");
    let just = maybe
        .constructor(&Symbol::qualified("maybe", "Just"))
        .expect("Just constructor");
    let payload = cx.factory().string("bound".to_owned()).unwrap();
    let value = just.construct(&mut cx, vec![payload]).unwrap();

    let matched = match_value(
        &mut cx,
        value,
        &[
            MatchArm::for_constructor(&nothing),
            MatchArm::for_constructor(&just),
        ],
    )
    .unwrap();

    assert_eq!(matched.arm_index(), 1);
    assert_eq!(matched.label(), &Symbol::qualified("maybe", "Just"));
    let capture = &matched.captures().values()[0].1;
    assert_eq!(
        capture.object().as_expr(&mut cx).unwrap(),
        Expr::String("bound".to_owned())
    );
}

#[test]
fn non_exhaustive_match_reports_a_diagnostic() {
    let maybe = maybe_type();
    let just = maybe
        .constructor(&Symbol::qualified("maybe", "Just"))
        .expect("Just constructor");
    let diagnostics = exhaustiveness_diagnostics(&maybe, &[MatchArm::for_constructor(&just)]);

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].code,
        Some(Symbol::qualified("pattern", "non-exhaustive"))
    );
    assert!(diagnostics[0].message.contains("maybe/Nothing"));
}

#[test]
fn destructuring_reuses_shape() {
    let mut cx = cx();
    let variant = VariantConstructor::new(
        Symbol::qualified("adt", "StringBox"),
        VariantDeclaration::new(
            Symbol::qualified("box", "StringBox"),
            vec![PatternField::new(
                Symbol::new("value"),
                Arc::new(ExprKindShape::new(sim_kernel::ExprKind::String)),
            )],
        ),
    );
    let shape = variant.shape();
    let accepted = destructure_expr(
        &mut cx,
        &Expr::Call {
            operator: Box::new(Expr::Symbol(Symbol::qualified("box", "StringBox"))),
            args: vec![Expr::String("text".to_owned())],
        },
        shape.as_ref(),
    )
    .unwrap();
    let rejected = destructure_expr(
        &mut cx,
        &Expr::Call {
            operator: Box::new(Expr::Symbol(Symbol::qualified("box", "StringBox"))),
            args: vec![Expr::Bool(true)],
        },
        shape.as_ref(),
    )
    .unwrap();

    assert!(accepted.accepted);
    assert!(!rejected.accepted);
}

#[test]
fn pattern_organ_claims_project_to_card() {
    let mut cx = cx();
    publish_pattern_organ_claims(&mut cx).unwrap();

    let claims = cx
        .query_facts(sim_kernel::ClaimPattern {
            subject: Some(Ref::Symbol(pattern_organ_symbol())),
            predicate: Some(card_kind_predicate()),
            object: Some(Ref::Symbol(standard_organ_kind())),
            include_revoked: false,
        })
        .unwrap();
    assert_eq!(claims.len(), 1);

    let card = card_for_ref(&mut cx, Ref::Symbol(pattern_organ_symbol())).unwrap();
    let table = card.object().as_table(&mut cx).unwrap();
    let entries = table.object().as_table_impl().unwrap();
    let ops = entries.get(&mut cx, Symbol::new("ops")).unwrap();
    let list = ops.object().as_list().unwrap();
    let values = force_list_to_vec(&mut cx, list, "pattern organ ops").unwrap();

    assert!(values.into_iter().any(|value| {
        value.object().as_expr(&mut cx).unwrap()
            == Expr::Symbol(Symbol::qualified("pattern", "match.v1"))
    }));
}
