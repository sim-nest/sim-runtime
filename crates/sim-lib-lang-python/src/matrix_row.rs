use crate::python_core_profile;
use sim_kernel::Symbol;
use sim_lib_standard_core::{
    LanguageRow, LanguageRowBuilder, SourceConformanceCase, SourceConformanceCaseKind,
    SourceExpectation,
};

/// Build the Python core conformance row from source-level cases.
pub fn python_core_matrix_row() -> LanguageRow {
    LanguageRowBuilder::new(Symbol::new("python"), python_core_profile())
        .with_cases(python_core_source_cases())
        .build()
}

/// Checked source cases entering through `codec/python`.
pub fn python_core_source_cases() -> Vec<SourceConformanceCase> {
    [
        ("scalar-flow", "answer = 0\nfor n in range(7):\n    if n % 2:\n        answer = answer + n\nanswer\n", "9"),
        ("closure", "def counter(start: int):\n    value = start\n    def step(delta: int = 1):\n        nonlocal value\n        value = value + delta\n        return value\n    return step\nf = counter(40)\nf() + f()\n", "83"),
        ("containers", "values = [1, 2, 3]\nindex = {'answer': values[0] + values[1] + values[2]}\nindex['answer']\n", "6"),
        ("objects-c3-descriptors-super", "class Root:\n    answer = 40\nclass Add(Root):\n    @property\n    def value(self): return super().answer + 2\nAdd().value\n", "42"),
        ("exception-context-cleanup", "try:\n    raise ValueError('root')\nexcept ValueError as cause:\n    try:\n        raise RuntimeError('outer') from cause\n    finally:\n        answer = 42\nanswer\n", "42"),
        ("generator-send-close", "def values():\n    item = yield 1\n    try:\n        yield item\n    finally:\n        cleanup = 42\ng = values()\nnext(g)\ng.send(42)\ng.close()\ncleanup\n", "42"),
    ].into_iter().map(|(name, source, expected)| SourceConformanceCase {
        symbol: Symbol::qualified("test/python-core", name),
        organ: Symbol::qualified("codec", "python"),
        source_name: format!("{name}.py"), source: source.to_owned(),
        kind: SourceConformanceCaseKind::Observed,
        expectation: SourceExpectation::LowersTo(expected.to_owned()), affects_badge: None,
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn row_is_source_backed() {
        let row = python_core_matrix_row();
        assert_eq!(row.language, Symbol::new("python"));
        assert_eq!(row.cases.len(), 6);
        assert!(
            row.cases
                .iter()
                .all(|case| case.organ == Symbol::qualified("codec", "python"))
        );
    }
}
