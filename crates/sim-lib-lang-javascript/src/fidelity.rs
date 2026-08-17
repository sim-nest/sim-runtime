//! Frozen, inspectable fidelity and regression evidence for the JavaScript profile.

/// Independent fidelity dimension.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JavascriptFidelityDimension {
    /// Stable dimension name.
    pub name: &'static str,
    /// Exact evidence boundary.
    pub evidence: &'static str,
}

/// Regression obligation tied to a concrete checked behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JavascriptRegressionCase {
    /// Stable case id.
    pub id: &'static str,
    /// Source module whose tests prove the behavior.
    pub evidence: &'static str,
}

/// Frozen specification authority; an oracle citation, never an implementation.
pub const ECMA262_ORACLE: &str = "ECMA-262, 17th edition (ECMAScript 2026), frozen 2026-08-01";
/// Frozen Test262 observation. No Test262 code is compiled or shipped.
pub const TEST262_ORACLE: &str = "tc39/test262@main as observed 2026-08-01";

/// Separately scored profile dimensions.
pub const fn javascript_fidelity_dimensions() -> &'static [JavascriptFidelityDimension] {
    &[
        JavascriptFidelityDimension {
            name: "syntax",
            evidence: "codec/javascript lossless Script and Module trees",
        },
        JavascriptFidelityDimension {
            name: "lowering",
            evidence: "canonical javascript/* expressions",
        },
        JavascriptFidelityDimension {
            name: "direct-evaluation",
            evidence: "bounded direct evaluator; no compiler or VM",
        },
        JavascriptFidelityDimension {
            name: "objects",
            evidence: "ordinary properties, descriptors, prototypes, and callable policy",
        },
        JavascriptFidelityDimension {
            name: "intrinsics",
            evidence: "generated intrinsics.tsv manifest with named shared backing",
        },
        JavascriptFidelityDimension {
            name: "jobs-modules",
            evidence: "explicit drain-to-empty microtasks and authorized source modules",
        },
        JavascriptFidelityDimension {
            name: "boundedness",
            evidence: "source, evaluation, arena, collection, pattern, job, and module limits",
        },
        JavascriptFidelityDimension {
            name: "expected-gaps",
            evidence: "profile and RegExp gap catalogs; Node and ambient hosts excluded",
        },
    ]
}

/// Generated regression inventory. Each named module contains its executable test.
pub const fn javascript_regression_cases() -> &'static [JavascriptRegressionCase] {
    &[
        JavascriptRegressionCase {
            id: "descriptors",
            evidence: "objects::tests",
        },
        JavascriptRegressionCase {
            id: "cycles",
            evidence: "managed::tests::shared_collector_reclaims_cycles",
        },
        JavascriptRegressionCase {
            id: "completion",
            evidence: "runtime::tests",
        },
        JavascriptRegressionCase {
            id: "collection",
            evidence: "managed::tests",
        },
        JavascriptRegressionCase {
            id: "drain-to-empty-jobs",
            evidence: "jobs::tests::checkpoint_drains_reentrant_microtasks_and_isolates_finalization",
        },
        JavascriptRegressionCase {
            id: "modules",
            evidence: "modules::tests",
        },
        JavascriptRegressionCase {
            id: "regexp-subset",
            evidence: "regexp::tests",
        },
        JavascriptRegressionCase {
            id: "utf16-code-unit-laws",
            evidence: "text::tests",
        },
        JavascriptRegressionCase {
            id: "capability-refusal",
            evidence: "modules::tests",
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fidelity_dimensions_and_regressions_are_frozen_separately() {
        assert_eq!(javascript_fidelity_dimensions().len(), 8);
        assert_eq!(javascript_regression_cases().len(), 9);
        assert!(
            javascript_fidelity_dimensions()
                .iter()
                .all(|x| !x.evidence.is_empty())
        );
        assert!(
            javascript_regression_cases()
                .iter()
                .all(|x| !x.evidence.is_empty())
        );
    }

    #[test]
    fn oracle_is_evidence_not_a_dependency() {
        let manifest = include_str!("../Cargo.toml").to_ascii_lowercase();
        for forbidden in [
            "test262",
            "swc",
            "boa_engine",
            "deno_core",
            "nodejs",
            "quickjs",
            "v8",
        ] {
            assert!(
                !manifest.contains(forbidden),
                "forbidden dependency {forbidden}"
            );
        }
        assert!(ECMA262_ORACLE.contains("frozen"));
        assert!(TEST262_ORACLE.contains("observed"));
    }

    #[test]
    fn sources_have_no_private_engine_fallback() {
        let sources = [
            include_str!("runtime.rs"),
            include_str!("objects/function.rs"),
            include_str!("objects/space.rs"),
            include_str!("objects/tests.rs"),
            include_str!("jobs.rs"),
            include_str!("modules.rs"),
            include_str!("managed.rs"),
            include_str!("profile.rs"),
        ]
        .join("\n")
        .to_ascii_lowercase();
        for forbidden in [
            "compilerir",
            "bytecodeop",
            "optimizerpass",
            "jitcompile",
            "privateheap",
            "privatescheduler",
            "modulecache",
            "nodefallback",
        ] {
            assert!(
                !sources.contains(forbidden),
                "forbidden engine seam {forbidden}"
            );
        }
    }
}
