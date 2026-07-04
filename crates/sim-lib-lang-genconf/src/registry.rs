//! Registry for generated language-conformance rows.

use sim_kernel::{Cx, Symbol};

use crate::{ExprSpace, GeneratedCoverageReport, run_generated_row};

/// One generated conformance row: language id, codec id, and expression space.
pub struct GenerativeRow {
    /// Language row measured by this generated conformance entry.
    pub language: Symbol,
    /// Codec for encoding and decoding generated expressions for this row.
    pub codec: Symbol,
    /// Expression space sampled for this row.
    pub space: ExprSpace,
}

/// Returns every language codec measured by generated conformance.
///
/// Adding a language is adding one row here with the language row id, reader
/// codec id, and shared expression space.
pub fn generative_registry() -> Vec<GenerativeRow> {
    vec![
        row("scheme", "scheme-r7rs-small"),
        row("common-lisp", "common-lisp-lite"),
        row("clojure", "clojure-edn"),
        row("islisp", "islisp"),
        row("julia", "algol"),
        row("lua", "algol"),
        row("ruby", "algol"),
        row("typed-lazy", "algol"),
        core_round_trip_row("prolog", "lisp"),
    ]
}

/// Runs generated conformance for every registered language row.
pub fn run_all_generated(cx: &mut Cx, budget: usize) -> Vec<GeneratedCoverageReport> {
    generative_registry()
        .into_iter()
        .map(|row| run_generated_row(cx, &row.language, &row.codec, &row.space, budget))
        .collect()
}

fn row(language: &str, codec: &str) -> GenerativeRow {
    GenerativeRow {
        language: Symbol::new(language),
        codec: Symbol::qualified("codec", codec),
        space: ExprSpace::r7rs_core_space(3),
    }
}

fn core_round_trip_row(language: &str, codec: &str) -> GenerativeRow {
    GenerativeRow {
        language: Symbol::new(language),
        codec: Symbol::qualified("codec", codec),
        space: ExprSpace::core_round_trip_space(3),
    }
}
