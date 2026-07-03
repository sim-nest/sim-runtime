//! Prolog conformance matrix row.

use sim_kernel::Symbol;
use sim_lib_standard_core::{
    LanguageRow, LanguageRowBuilder, SourceConformanceCase, SourceExpectation,
};

use crate::{
    prolog_conformance_case_symbol, prolog_logic_organ_symbol, prolog_profile,
    prolog_surface_fidelity_symbol,
};

/// Builds the Prolog surface matrix row.
pub fn prolog_matrix_row() -> LanguageRow {
    LanguageRowBuilder::new(Symbol::new("prolog"), prolog_profile())
        .with_cases(prolog_conformance_cases())
        .build()
}

/// Minimal source cases for the Prolog surface matrix row.
pub fn prolog_conformance_cases() -> Vec<SourceConformanceCase> {
    let mut cases = vec![
        prolog_case(
            "fact",
            "fact-color.prolog",
            "color(red).",
            "prolog:fact answers=1",
        ),
        prolog_case(
            "rule",
            "rule-painted.prolog",
            "painted(X) :- color(X).",
            "prolog:rule answers=1",
        ),
        prolog_case(
            "query",
            "query-color.prolog",
            "?- color(X).",
            "prolog:query answers=2",
        ),
        prolog_case(
            "cut",
            "cut-first-color.prolog",
            "first_color(X) :- color(X), !.",
            "prolog:cut answers=1 first=red",
        ),
    ];
    cases.extend(prolog_arithmetic_cases());
    cases.extend(prolog_list_cases());
    cases.extend(prolog_all_solution_cases());
    cases.extend(prolog_constraint_cases());
    cases.extend(prolog_tabling_cases());
    cases
}

fn prolog_case(
    name: &str,
    source_name: &str,
    source: &str,
    expected: &str,
) -> SourceConformanceCase {
    SourceConformanceCase {
        symbol: prolog_conformance_case_symbol(name),
        organ: prolog_logic_organ_symbol(),
        source_name: source_name.to_owned(),
        source: source.to_owned(),
        expectation: SourceExpectation::LowersTo(expected.to_owned()),
        affects_badge: Some(prolog_surface_fidelity_symbol()),
    }
}

fn prolog_numbers_case(
    name: &str,
    source_name: &str,
    source: &str,
    expected: &str,
) -> SourceConformanceCase {
    SourceConformanceCase {
        symbol: prolog_conformance_case_symbol(name),
        organ: Symbol::qualified("numbers", "arith"),
        source_name: source_name.to_owned(),
        source: source.to_owned(),
        expectation: SourceExpectation::LowersTo(expected.to_owned()),
        affects_badge: Some(prolog_surface_fidelity_symbol()),
    }
}

fn prolog_numbers_gap(
    name: &str,
    source_name: &str,
    source: &str,
    code: Symbol,
    reason: &str,
) -> SourceConformanceCase {
    SourceConformanceCase {
        symbol: prolog_conformance_case_symbol(name),
        organ: Symbol::qualified("numbers", "arith"),
        source_name: source_name.to_owned(),
        source: source.to_owned(),
        expectation: SourceExpectation::ExpectedGap {
            code,
            reason: reason.to_owned(),
        },
        affects_badge: Some(prolog_surface_fidelity_symbol()),
    }
}

fn prolog_sequence_case(
    name: &str,
    source_name: &str,
    source: &str,
    expected: &str,
) -> SourceConformanceCase {
    SourceConformanceCase {
        symbol: prolog_conformance_case_symbol(name),
        organ: Symbol::new("sequence"),
        source_name: source_name.to_owned(),
        source: source.to_owned(),
        expectation: SourceExpectation::LowersTo(expected.to_owned()),
        affects_badge: Some(prolog_surface_fidelity_symbol()),
    }
}

fn prolog_sequence_gap(
    name: &str,
    source_name: &str,
    source: &str,
    code: Symbol,
    reason: &str,
) -> SourceConformanceCase {
    SourceConformanceCase {
        symbol: prolog_conformance_case_symbol(name),
        organ: Symbol::new("sequence"),
        source_name: source_name.to_owned(),
        source: source.to_owned(),
        expectation: SourceExpectation::ExpectedGap {
            code,
            reason: reason.to_owned(),
        },
        affects_badge: Some(prolog_surface_fidelity_symbol()),
    }
}

fn prolog_control_case(
    name: &str,
    source_name: &str,
    source: &str,
    expected: &str,
) -> SourceConformanceCase {
    SourceConformanceCase {
        symbol: prolog_conformance_case_symbol(name),
        organ: Symbol::new("control"),
        source_name: source_name.to_owned(),
        source: source.to_owned(),
        expectation: SourceExpectation::LowersTo(expected.to_owned()),
        affects_badge: Some(prolog_surface_fidelity_symbol()),
    }
}

fn prolog_control_gap(
    name: &str,
    source_name: &str,
    source: &str,
    code: Symbol,
    reason: &str,
) -> SourceConformanceCase {
    SourceConformanceCase {
        symbol: prolog_conformance_case_symbol(name),
        organ: Symbol::new("control"),
        source_name: source_name.to_owned(),
        source: source.to_owned(),
        expectation: SourceExpectation::ExpectedGap {
            code,
            reason: reason.to_owned(),
        },
        affects_badge: Some(prolog_surface_fidelity_symbol()),
    }
}

fn prolog_arithmetic_cases() -> Vec<SourceConformanceCase> {
    vec![
        prolog_numbers_case(
            "is-promote",
            "is-promote.prolog",
            "X is 1 + 0.5.",
            "prolog:is organ=numbers/arith answers=1 x=1.5",
        ),
        prolog_numbers_case(
            "cmp-cross-domain",
            "cmp-cross-domain.prolog",
            "?- 2 =:= 2.0.",
            "prolog:compare organ=numbers/arith answers=1",
        ),
        prolog_numbers_case(
            "cmp-false",
            "cmp-false.prolog",
            "?- 3 < 2.",
            "prolog:compare answers=0",
        ),
        prolog_numbers_gap(
            "unbound-is",
            "unbound-is.prolog",
            "X is Y + 1.",
            Symbol::qualified("prolog", "unbound-arithmetic"),
            "is/2 requires the right side to be ground and evaluable",
        ),
    ]
}

fn prolog_list_cases() -> Vec<SourceConformanceCase> {
    vec![
        prolog_sequence_case(
            "list-member",
            "list-member.prolog",
            "?- member(X, [a,b,c]).",
            "prolog:member organ=sequence answers=3 xs=a,b,c",
        ),
        prolog_sequence_case(
            "list-append",
            "list-append.prolog",
            "?- append([a], [b,c], Xs).",
            "prolog:append organ=sequence answers=1 xs=(a b c)",
        ),
        prolog_sequence_gap(
            "open-list",
            "open-list.prolog",
            "?- member(X, [a|Tail]).",
            Symbol::qualified("prolog", "open-list"),
            "open and improper lists are outside the closed Expr::List bridge",
        ),
    ]
}

fn prolog_all_solution_cases() -> Vec<SourceConformanceCase> {
    vec![
        prolog_sequence_case(
            "findall-duplicates",
            "findall-duplicates.prolog",
            "findall(X, member(X, [a,b,a]), Xs).",
            "prolog:findall organ=sequence answers=1 xs=(a b a)",
        ),
        prolog_sequence_case(
            "bagof-groups",
            "bagof-groups.prolog",
            "bagof(Child, parent(Parent, Child), Children).",
            "prolog:bagof organ=sequence answers=2 groups=alice:(bob bea);cara:(drew)",
        ),
        prolog_sequence_case(
            "setof-sorted",
            "setof-sorted.prolog",
            "setof(X, member(X, [c,a,b,a]), Xs).",
            "prolog:setof organ=sequence answers=1 xs=(a b c)",
        ),
        prolog_sequence_case(
            "bagof-empty",
            "bagof-empty.prolog",
            "bagof(X, member(X, []), Xs).",
            "prolog:bagof-empty organ=sequence answers=0",
        ),
    ]
}

fn prolog_constraint_cases() -> Vec<SourceConformanceCase> {
    vec![
        prolog_control_case(
            "constraint-entailed",
            "constraint-entailed.prolog",
            "?- #=(2, 2).",
            "prolog:constraint organ=control relation=#= verdict=entailed answers=1",
        ),
        prolog_control_case(
            "constraint-disentailed",
            "constraint-disentailed.prolog",
            "?- #<(3, 2).",
            "prolog:constraint organ=control relation=#< verdict=disentailed answers=0",
        ),
        prolog_control_gap(
            "constraint-residual",
            "constraint-residual.prolog",
            "?- dif(X, 1).",
            Symbol::qualified("prolog", "residual-constraint"),
            "residual constraint demand is suspended on the control ledger",
        ),
    ]
}

fn prolog_tabling_cases() -> Vec<SourceConformanceCase> {
    vec![prolog_sequence_case(
        "tabling-left-recursive-path",
        "tabling-left-recursive-path.prolog",
        "table(path/2), path(a, Y).",
        "prolog:tabling organ=sequence answers=2 ys=b,c",
    )]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prolog_matrix_row_language_symbol_is_prolog() {
        let row = prolog_matrix_row();

        assert_eq!(row.language, Symbol::new("prolog"));
        assert!(!row.is_empty());
        assert_eq!(row.cases.len(), 19);
        assert!(
            row.cases
                .iter()
                .filter(|case| matches!(case.expectation, SourceExpectation::LowersTo(_)))
                .count()
                == 16
        );
        assert!(
            row.cases
                .iter()
                .any(|case| matches!(case.expectation, SourceExpectation::ExpectedGap { .. }))
        );
    }
}
