use std::collections::BTreeSet;

use sim_kernel::Symbol;

use crate::language_matrix;

#[test]
fn language_matrix_has_exactly_nine_rows() {
    let matrix = language_matrix();

    assert_eq!(matrix.language_count(), 9);
}

#[test]
fn language_matrix_has_no_duplicate_row_names() {
    let matrix = language_matrix();
    let names: Vec<_> = matrix
        .iter_rows()
        .map(|row| row.language.to_string())
        .collect();
    let unique: BTreeSet<_> = names.iter().cloned().collect();

    assert_eq!(unique.len(), names.len());
}

#[test]
fn language_matrix_rows_are_populated() {
    let matrix = language_matrix();

    for language in [
        "scheme",
        "common-lisp",
        "clojure",
        "islisp",
        "julia",
        "lua",
        "ruby",
        "typed-lazy",
        "prolog",
    ] {
        let row = matrix
            .row(&Symbol::new(language))
            .unwrap_or_else(|| panic!("{language} row must be registered"));
        assert!(!row.is_empty(), "{language} row must have cases");
    }
}
