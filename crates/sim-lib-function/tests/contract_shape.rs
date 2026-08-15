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
fn manifest_does_not_depend_on_generic_dispatch() {
    let manifest = fs::read_to_string(crate_root().join("Cargo.toml")).unwrap();
    assert!(!manifest.contains("sim-lib-dispatch"));
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
