use std::collections::BTreeSet;
use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, EagerPolicy, Symbol};
use sim_lib_lang_genconf::{generative_registry, run_all_generated};

use crate::language_matrix;

fn closure_cx() -> Cx {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    sim_test_support::register_core_classes(&mut cx);
    sim_test_support::register_f64_number_domain(&mut cx);
    cx
}

#[test]
fn closure_registry_tracks_the_curated_language_matrix() {
    let registry = generative_registry();
    let matrix = language_matrix();
    let mut names = BTreeSet::new();

    assert_eq!(registry.len(), matrix.language_count());
    assert_eq!(matrix.language_count(), 9);
    for row in &registry {
        assert!(
            names.insert(row.language.clone()),
            "duplicate generated row: {}",
            row.language,
        );
        assert!(
            matrix.row(&row.language).is_some(),
            "{} row must stay in the curated matrix",
            row.language,
        );
    }
}

#[test]
fn closure_registry_uses_live_language_codec_ids() {
    let registry = generative_registry();

    assert!(registry.iter().any(|row| {
        row.language == Symbol::new("scheme")
            && row.codec == Symbol::qualified("codec", "scheme-r7rs-small")
    }));
    assert!(registry.iter().any(|row| {
        row.language == Symbol::new("common-lisp")
            && row.codec == Symbol::qualified("codec", "common-lisp-lite")
    }));
    assert!(registry.iter().any(|row| {
        row.language == Symbol::new("prolog") && row.codec == Symbol::qualified("codec", "lisp")
    }));
}

#[test]
fn closure_reports_are_anchored_or_honest() {
    let mut cx = closure_cx();
    let reports = run_all_generated(&mut cx, 8);

    assert_eq!(reports.len(), generative_registry().len());
    for report in &reports {
        if !report.landmark_reproduced() {
            assert!(
                report.coverage_percent().is_none(),
                "{} must not publish unanchored coverage",
                report.language,
            );
        }
    }
    assert_eq!(language_matrix().language_count(), 9);
}
