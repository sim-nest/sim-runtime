// conformance: function contracts retain the shared Shape boundary.

use std::{fs, path::PathBuf};

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn production_surface_cannot_erase_the_guest_body() {
    let source = fs::read_to_string(crate_root().join("src/lib.rs")).unwrap();
    let forbidden = ["dyn Any", "Box < dyn Any", "Box<dyn Any>"];
    for spelling in forbidden {
        assert!(
            !source
                .lines()
                .filter(|line| !line.starts_with("//!"))
                .any(|line| line.contains(spelling)),
            "production API erases its body through {spelling}"
        );
    }
}

#[test]
fn production_surface_has_no_global_body_registry() {
    let source = fs::read_to_string(crate_root().join("src/lib.rs")).unwrap();
    for spelling in ["BodyRegistry", "BODY_REGISTRY", "body_registry"] {
        assert!(
            !source.contains(spelling),
            "global body registry marker {spelling}"
        );
    }
}

#[test]
fn generic_dispatch_is_confined_to_the_opt_in_adapter() {
    let instance = fs::read_to_string(crate_root().join("src/instance.rs")).unwrap();
    let adapter = fs::read_to_string(crate_root().join("src/callable.rs")).unwrap();
    assert!(!instance.contains("GenericFunction"));
    assert!(!instance.contains("DispatchMethod"));
    assert!(adapter.contains("dispatch_method_body"));
}

#[test]
fn neutral_binding_has_no_language_decision_tables() {
    let source = fs::read_to_string(crate_root().join("src/bind.rs")).unwrap();
    for spelling in ["default_value", "defaulted", "keyword_precedence"] {
        assert!(
            !source.contains(spelling),
            "neutral binding contains language decision marker {spelling}"
        );
    }
}
