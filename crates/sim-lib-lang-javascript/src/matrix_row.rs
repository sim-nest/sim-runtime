use sim_kernel::Symbol;
use sim_lib_standard_core::{
    LanguageRow, LanguageRowBuilder, SourceConformanceCase, SourceConformanceCaseKind,
    SourceExpectation,
};
/// Build the JavaScript core conformance row.
pub fn javascript_core_matrix_row() -> LanguageRow {
    LanguageRowBuilder::new(Symbol::new("javascript"), crate::javascript_core_profile())
        .with_cases(javascript_core_source_cases())
        .build()
}
/// Checked source cases entering only through `codec/javascript`.
pub fn javascript_core_source_cases() -> Vec<SourceConformanceCase> {
    [
        (
            "primitive-coercion-equality",
            "let answer = '40' == 40; answer;",
            "true",
        ),
        (
            "declaration-loop",
            "let answer = 0; while (answer < 42) { answer += 1; } answer;",
            "42",
        ),
        (
            "destructuring-abrupt",
            "let [left, right] = [40, 2]; return left + right;",
            "return 42",
        ),
        ("installed-bigint", "let answer = 40n + 2n; answer;", "42n"),
    ]
    .into_iter()
    .map(|(name, source, expected)| SourceConformanceCase {
        symbol: Symbol::qualified("test/javascript-core", name),
        organ: Symbol::qualified("codec", "javascript"),
        source_name: format!("{name}.js"),
        source: source.into(),
        kind: SourceConformanceCaseKind::Observed,
        expectation: SourceExpectation::LowersTo(expected.into()),
        affects_badge: None,
    })
    .collect()
}
#[cfg(test)]
mod tests {
    #[test]
    fn row_is_codec_source_backed() {
        let row = super::javascript_core_matrix_row();
        assert_eq!(row.cases.len(), 4);
        assert!(
            row.cases
                .iter()
                .all(|c| c.organ == sim_kernel::Symbol::qualified("codec", "javascript"))
        );
    }
}
