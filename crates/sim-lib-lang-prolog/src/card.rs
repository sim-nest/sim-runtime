//! Prolog profile Card helpers.

use sim_kernel::{
    Cx, Ref, Result, Symbol, Value, card::card_for_ref_with_fallback,
    standard::standard_profile_kind,
};
use sim_lib_logic::builtins::{BuiltinTable, tabling_memo_binding};
use sim_lib_standard_core::{MatrixRunReport, standard_test_capability};

use crate::{prolog_profile_symbol, run_prolog_matrix_row};

#[cfg(feature = "generated-coverage")]
use crate::generated_coverage::prolog_generated_coverage_card_fields;

/// Builds the Prolog profile Card with browseable conformance fields.
///
/// When the context lacks `standard.test`, the card reports zero counts and
/// `conformance.fidelity = "unscored"` instead of running the harness.
pub fn prolog_language_card(cx: &mut Cx) -> Result<Value> {
    let language = prolog_language_symbol();
    let mut entries = vec![
        (
            Symbol::new("profile"),
            cx.factory().symbol(prolog_profile_symbol())?,
        ),
        (
            Symbol::new("language"),
            cx.factory().symbol(language.clone())?,
        ),
    ];
    entries.extend(prolog_conformance_fields(cx, &language)?);
    entries.extend(prolog_generated_coverage_fields(cx)?);
    entries.extend(prolog_builtin_organ_fields(cx)?);
    let fallback = cx.factory().table(entries)?;
    card_for_ref_with_fallback(
        cx,
        Ref::Symbol(prolog_profile_symbol()),
        Some(fallback),
        Some(standard_profile_kind()),
    )
}

fn prolog_builtin_organ_fields(cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
    let mut table = BuiltinTable::standard();
    table.register(tabling_memo_binding(Symbol::new("path")));
    let keys = [
        "is", "findall", "bagof", "setof", "member", "append", "length", "select", "#=", "#<",
        "dif", "path",
    ];
    keys.into_iter()
        .filter_map(|key| {
            table
                .organ_of(&Symbol::new(key))
                .cloned()
                .map(|organ| (builtin_organ_field(key), organ))
        })
        .map(|(field, organ)| Ok((field, cx.factory().symbol(organ)?)))
        .collect()
}

fn builtin_organ_field(key: &str) -> Symbol {
    let key = match key {
        "#=" => "constraint.eq",
        "#<" => "constraint.lt",
        "path" => "tabling.path",
        other => other,
    };
    Symbol::new(format!("builtin.{key}.organ"))
}

fn prolog_conformance_fields(cx: &mut Cx, language: &Symbol) -> Result<Vec<(Symbol, Value)>> {
    if cx.require(&standard_test_capability()).is_ok() {
        return run_prolog_matrix_row(cx)?.conformance_card_fields(cx, language);
    }
    MatrixRunReport::unscored_conformance_card_fields(cx)
}

fn prolog_generated_coverage_fields(cx: &mut Cx) -> Result<Vec<(Symbol, Value)>> {
    #[cfg(feature = "generated-coverage")]
    if cx.require(&standard_test_capability()).is_ok() {
        return prolog_generated_coverage_card_fields(cx);
    }
    #[cfg(not(feature = "generated-coverage"))]
    let _ = cx;
    Ok(Vec::new())
}

fn prolog_language_symbol() -> Symbol {
    Symbol::new("prolog")
}

#[cfg(test)]
mod tests {
    use sim_kernel::{Expr, NumberLiteral, testing::bare_cx as cx};
    use sim_lib_standard_core::standard_test_capability;

    use super::*;

    #[test]
    fn prolog_card_without_capability_emits_unscored() {
        let mut cx = cx();

        let card = prolog_language_card(&mut cx).unwrap();
        let expr = card.object().as_expr(&mut cx).unwrap();

        assert_eq!(number_table_value(&expr, "conformance.pass"), Some("0"));
        assert_eq!(number_table_value(&expr, "conformance.gap"), Some("0"));
        assert_eq!(number_table_value(&expr, "conformance.fail"), Some("0"));
        assert_eq!(
            table_value(&expr, "conformance.fidelity"),
            Some(&Expr::String("unscored".to_owned()))
        );
    }

    #[test]
    fn prolog_card_with_capability_emits_fidelity() {
        let mut cx = cx();
        cx.grant(standard_test_capability());

        let card = prolog_language_card(&mut cx).unwrap();
        let expr = card.object().as_expr(&mut cx).unwrap();

        assert_eq!(number_table_value(&expr, "conformance.pass"), Some("16"));
        assert_eq!(number_table_value(&expr, "conformance.gap"), Some("3"));
        assert_eq!(number_table_value(&expr, "conformance.fail"), Some("0"));
        assert_eq!(
            table_value(&expr, "conformance.fidelity"),
            Some(&Expr::String("100%".to_owned()))
        );
        assert_eq!(
            table_value(&expr, "builtin.is.organ"),
            Some(&Expr::Symbol(Symbol::qualified("numbers", "arith")))
        );
        assert_eq!(
            table_value(&expr, "builtin.tabling.path.organ"),
            Some(&Expr::Symbol(Symbol::new("sequence")))
        );
        assert_eq!(table_value(&expr, "coverage.generated.percent"), None);
        assert_eq!(table_value(&expr, "coverage.generated.citation"), None);
    }

    fn table_value<'a>(expr: &'a Expr, key: &str) -> Option<&'a Expr> {
        let Expr::Map(entries) = expr else {
            return None;
        };
        entries.iter().find_map(|(entry_key, entry_value)| {
            let Expr::Symbol(entry_key) = entry_key else {
                return None;
            };
            (entry_key == &Symbol::new(key)).then_some(entry_value)
        })
    }

    fn number_table_value<'a>(expr: &'a Expr, key: &str) -> Option<&'a str> {
        let value = table_value(expr, key)?;
        let Expr::Number(NumberLiteral { domain, canonical }) = value else {
            return None;
        };
        assert_eq!(domain, &Symbol::qualified("numbers", "u64"));
        Some(canonical.as_str())
    }
}
