//! Generated expression round-trip properties.

use sim_codec::{Input, decode_with_codec, encode_with_codec};
use sim_kernel::{Cx, EncodeOptions, Expr, ReadPolicy, Symbol};
use sim_lib_standard_core::{ExprRoundTripCase, ExprRoundTripObservation};

use crate::space::ExprSpace;

/// Checks the generated round-trip property for one expression and codec.
///
/// The property is the ROUNDTRIP_4 path: encode an expression through the codec,
/// read the rendered source back with the same codec, and compare the two
/// expression graphs with canonical equality.
pub fn check_round_trip(cx: &mut Cx, codec: &Symbol, expr: &Expr) -> ExprRoundTripObservation {
    let out = match encode_with_codec(cx, codec, expr, EncodeOptions::default()) {
        Ok(out) => out,
        Err(err) => {
            return ExprRoundTripObservation::Diagnostic(Symbol::qualified(
                "codec",
                diagnostic_slug(&err),
            ));
        }
    };
    let source = match out.into_text() {
        Ok(source) => source,
        Err(err) => {
            return ExprRoundTripObservation::Diagnostic(Symbol::qualified(
                "codec",
                diagnostic_slug(&err),
            ));
        }
    };
    let back = match decode_with_codec(cx, codec, Input::Text(source), ReadPolicy::default()) {
        Ok(expr) => expr,
        Err(err) => {
            return ExprRoundTripObservation::Diagnostic(Symbol::qualified(
                "codec",
                diagnostic_slug(&err),
            ));
        }
    };

    let expected = expr_display(expr);
    if expr.canonical_eq(&back) {
        ExprRoundTripObservation::RoundTripped(expected)
    } else {
        ExprRoundTripObservation::Mismatch {
            expected,
            got: expr_display(&back),
        }
    }
}

/// Projects generated expressions into matrix expression round-trip cases.
///
/// Each case stores codec-rendered source text when the codec can encode the
/// expression. Expressions outside that codec surface remain explicit generated
/// cases with fallback source text, so later runners observe a diagnostic or gap
/// instead of a silent pass. Generated cases never affect curated badges.
pub fn generated_expr_cases(
    cx: &mut Cx,
    language: &Symbol,
    codec: &Symbol,
    space: &ExprSpace,
    budget: usize,
) -> Vec<ExprRoundTripCase> {
    space
        .enumerate(budget)
        .into_iter()
        .enumerate()
        .map(|(index, expr)| ExprRoundTripCase {
            symbol: Symbol::qualified(
                format!("gen/{}", language.as_qualified_str()),
                format!("expr-{index}"),
            ),
            language: language.clone(),
            source: render_seed(cx, codec, &expr),
            expected_display: Some(expr_display(&expr)),
            affects_badge: None,
        })
        .collect()
}

fn render_seed(cx: &mut Cx, codec: &Symbol, expr: &Expr) -> String {
    match encode_with_codec(cx, codec, expr, EncodeOptions::default()) {
        Ok(out) => out.into_text().unwrap_or_else(|_| format!("{expr:?}")),
        Err(_) => format!("{expr:?}"),
    }
}

fn expr_display(expr: &Expr) -> String {
    format!("Expr::{expr:?}")
}

fn diagnostic_slug(err: &sim_kernel::Error) -> &'static str {
    let message = err.to_string().to_ascii_lowercase();
    if message.contains("unsupported") {
        "unsupported"
    } else if message.contains("no encoder") {
        "encode-unavailable"
    } else {
        "error"
    }
}

#[cfg(test)]
mod tests {
    use sim_codec_lisp::LispCodecLib;
    use sim_kernel::{DefaultFactory, EagerPolicy};
    use sim_lib_lang_scheme::{SchemeCodecLib, scheme_reader_symbol};
    use std::sync::Arc;

    use super::*;

    fn property_cx() -> Cx {
        let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
        sim_test_support::register_core_classes(&mut cx);
        sim_test_support::register_f64_number_domain(&mut cx);
        cx
    }

    fn register_lisp_codec(cx: &mut Cx) -> Symbol {
        let codec = Symbol::qualified("codec", "lisp");
        let lib = LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
        cx.load_lib(&lib).unwrap();
        codec
    }

    fn register_scheme_codec(cx: &mut Cx) -> Symbol {
        let codec = scheme_reader_symbol();
        let lib = SchemeCodecLib::new(cx.registry_mut().fresh_codec_id());
        cx.load_lib(&lib).unwrap();
        codec
    }

    #[test]
    fn round_trip_pass_returns_round_tripped() {
        let mut cx = property_cx();
        let codec = register_lisp_codec(&mut cx);
        let expr = Expr::Bool(true);

        let observation = check_round_trip(&mut cx, &codec, &expr);

        assert_eq!(
            observation,
            ExprRoundTripObservation::RoundTripped("Expr::Bool(true)".to_owned())
        );
    }

    #[test]
    fn round_trip_out_of_profile_expr_is_gap_or_diagnostic() {
        let mut cx = property_cx();
        let codec = register_scheme_codec(&mut cx);
        let expr = Expr::Bool(true);

        let observation = check_round_trip(&mut cx, &codec, &expr);

        assert!(
            matches!(
                observation,
                ExprRoundTripObservation::Gap(_) | ExprRoundTripObservation::Diagnostic(_)
            ),
            "out-of-profile expression silently passed: {observation:?}",
        );
    }

    #[test]
    fn generated_round_trip_cases_do_not_affect_badges() {
        let mut cx = property_cx();
        let codec = register_lisp_codec(&mut cx);
        let language = Symbol::new("lisp");
        let space = ExprSpace::r7rs_core_space(2);

        let cases = generated_expr_cases(&mut cx, &language, &codec, &space, 4);

        assert_eq!(cases.len(), 4);
        for (index, case) in cases.iter().enumerate() {
            assert_eq!(
                case.symbol,
                Symbol::qualified("gen/lisp", format!("expr-{index}"))
            );
            assert_eq!(case.language, language);
            assert!(case.affects_badge.is_none());
            assert!(
                case.expected_display
                    .as_deref()
                    .is_some_and(|s| { s.starts_with("Expr::") })
            );
            assert!(!case.source.is_empty());
        }
    }
}
