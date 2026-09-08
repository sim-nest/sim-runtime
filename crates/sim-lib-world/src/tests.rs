use std::time::{Duration, Instant};

use sim_kernel::{Datum, Export, Lib};

use super::*;

#[test]
fn bundled_product_projects_and_preserves_envelope_independence() {
    let product = WorldProduct::bundled().unwrap();
    let a = product
        .project(
            "world/public-api-v1",
            SOURCE_FACT,
            Datum::String("api-v1".to_owned()),
            Some(Datum::String("host/a attempt/1".to_owned())),
        )
        .unwrap();
    let b = product
        .project(
            "world/public-api-v1",
            SOURCE_FACT,
            Datum::String("api-v1".to_owned()),
            Some(Datum::String("host/b attempt/9".to_owned())),
        )
        .unwrap();
    assert_eq!(a.result.digest, b.result.digest);
    assert_eq!(a.result.affected.len(), 2);
    assert_eq!(product.effect_calls(), 0);
}

#[test]
fn disclosure_change_invalidates_only_its_consumer() {
    let product = WorldProduct::bundled().unwrap();
    let diff = product
        .diff(
            "no-v3/disclosure-policy-v1",
            DISCLOSURE_FACT,
            Datum::String("allowlist/a".to_owned()),
            Datum::String("allowlist/b".to_owned()),
        )
        .unwrap();
    let Datum::Node { fields, .. } = diff else {
        panic!("diff is not a record")
    };
    let affected = fields
        .iter()
        .find(|(name, _)| name.to_string() == "affected")
        .map(|(_, value)| value)
        .unwrap();
    assert_eq!(
        affected,
        &Datum::Vector(vec![Datum::String(DISCLOSURE_CONCLUSION.to_owned())])
    );
}

#[test]
fn why_is_exact_cached_and_has_no_effect_route() {
    let product = WorldProduct::bundled().unwrap();
    for _ in 0..20 {
        product.why(DISCLOSURE_CONCLUSION, DISCLOSURE_FACT).unwrap();
    }
    let mut samples = Vec::with_capacity(200);
    for _ in 0..200 {
        let started = Instant::now();
        let value = product.why(DISCLOSURE_CONCLUSION, DISCLOSURE_FACT).unwrap();
        samples.push(started.elapsed());
        assert!(matches!(value, Datum::Node { .. }));
    }
    samples.sort_unstable();
    let p95 = samples[189];
    assert!(p95 <= Duration::from_millis(200), "p95 was {p95:?}");
    assert_eq!(product.effect_calls(), 0);
}

#[test]
fn command_is_a_loaded_world_entrypoint_without_capabilities() {
    let command = WorldCommandLib::new().unwrap();
    let manifest = command.manifest();
    assert!(manifest.capabilities.is_empty());
    assert!(manifest.requires.is_empty());
    assert!(manifest.exports.iter().any(|export| matches!(
        export,
        Export::Function { symbol, .. } if symbol.to_string() == "cli/main/world"
    )));
}
