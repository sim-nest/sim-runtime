//! Frozen, independently inspectable evidence for the TypeScript notation profile.

/// One evidence dimension. These dimensions are deliberately not combined into
/// a compatibility score: in particular, this profile publishes no type-check
/// score because it contains no TypeScript checker.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TypeScriptFidelityDimension {
    /// Stable dimension name.
    pub name: &'static str,
    /// Exact checked boundary.
    pub evidence: &'static str,
}

/// Frozen external syntax/erasure oracle. The tool is observed outside the
/// product; none of its compiler, checker, parser, service, or project code is
/// linked, imported, embedded, or invoked by this crate.
pub const TYPESCRIPT_EXTERNAL_ORACLE: &str =
    "microsoft/TypeScript 7.0.2 (offline syntax and erasure oracle only)";

/// The six independent fidelity dimensions published by this notation profile.
pub const fn typescript_fidelity_dimensions() -> &'static [TypeScriptFidelityDimension] {
    &[
        TypeScriptFidelityDimension {
            name: "syntax",
            evidence: "codec/typescript bounded lossless TypeScript and TSX trees",
        },
        TypeScriptFidelityDimension {
            name: "erasure",
            evidence: "deterministic direct lowering of compiler-independent notation",
        },
        TypeScriptFidelityDimension {
            name: "shape-metadata",
            evidence: "faithful non-enforcing callable BrowseSignature projections",
        },
        TypeScriptFidelityDimension {
            name: "javascript-equivalence",
            evidence: "the unchanged JavaScript evaluator returns identical values and effects",
        },
        TypeScriptFidelityDimension {
            name: "boundedness",
            evidence: "codec source, token, node, depth, and lowering budgets plus eval fuel",
        },
        TypeScriptFidelityDimension {
            name: "expected-gaps",
            evidence: "checker-dependent and code-producing syntax fails closed",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_are_separate_complete_and_publish_no_type_check_score() {
        let dimensions = typescript_fidelity_dimensions();
        assert_eq!(dimensions.len(), 6);
        assert_eq!(
            dimensions.iter().map(|item| item.name).collect::<Vec<_>>(),
            [
                "syntax",
                "erasure",
                "shape-metadata",
                "javascript-equivalence",
                "boundedness",
                "expected-gaps",
            ]
        );
        assert!(dimensions.iter().all(|item| !item.evidence.is_empty()));
        assert!(
            dimensions
                .iter()
                .all(|item| !item.name.contains("type-check"))
        );
    }

    #[test]
    fn frozen_tool_is_an_external_oracle_not_a_dependency() {
        let manifest = include_str!("../Cargo.toml").to_ascii_lowercase();
        for forbidden in [
            "typescript =",
            "typescript-eslint",
            "swc",
            "oxc",
            "deno",
            "node",
        ] {
            assert!(
                !manifest.contains(forbidden),
                "forbidden dependency {forbidden}"
            );
        }
        assert!(TYPESCRIPT_EXTERNAL_ORACLE.contains("7.0.2"));
        assert!(TYPESCRIPT_EXTERNAL_ORACLE.contains("oracle only"));
    }
}
