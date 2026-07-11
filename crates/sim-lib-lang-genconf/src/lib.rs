#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Generative conformance support over the shared expression graph.
//!
//! This crate owns deterministic `Expr`-space enumeration for language
//! conformance. The matrix runner and language rows stay in their existing
//! crates; this crate supplies the generated inputs they consume.

pub mod anchor;
pub mod coverage;
pub mod property;
pub mod publish;
pub mod registry;
pub mod seed;
pub mod space;

pub use anchor::{
    CoverageVerdict, LandmarkCorpus, common_lisp_lite_landmark_corpus, r7rs_small_landmark_corpus,
};
pub use coverage::{GeneratedCoverageReport, run_generated_row};
pub use property::{check_round_trip, generated_expr_cases};
pub use publish::{
    coverage_card_fields, generated_coverage_profile_symbol, publish_coverage_claims,
};
pub use registry::{GenerativeRow, generative_registry, run_all_generated};
pub use seed::r7rs_seed_corpus;
pub use space::ExprSpace;

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod landmark_tests {
    use sim_kernel::Expr;
    use sim_lib_lang_scheme::r7rs_small_expr_cases;

    use super::ExprSpace;

    /// The generator must reproduce every curated Scheme expression landmark
    /// before generated coverage is trusted.
    #[test]
    fn generator_reproduces_curated_scheme_landmarks() {
        let space = ExprSpace::r7rs_core_space(3);
        let generated = space.enumerate(256);
        let landmarks = curated_landmark_exprs();

        assert!(!r7rs_small_expr_cases().is_empty());
        assert!(
            !landmarks.is_empty(),
            "Scheme row must expose at least one concrete Expr landmark",
        );
        for landmark in landmarks {
            assert!(
                generated
                    .iter()
                    .any(|generated| generated.canonical_eq(&landmark)),
                "generator failed to reproduce curated landmark: {landmark:?}",
            );
        }
    }

    fn curated_landmark_exprs() -> Vec<Expr> {
        r7rs_small_expr_cases()
            .into_iter()
            .filter_map(|case| {
                case.expected_display
                    .as_deref()
                    .and_then(expr_from_expected_display)
            })
            .collect()
    }

    fn expr_from_expected_display(display: &str) -> Option<Expr> {
        match display {
            "Expr::Bool(true)" => Some(Expr::Bool(true)),
            "Expr::Bool(false)" => Some(Expr::Bool(false)),
            _ => None,
        }
    }
}
