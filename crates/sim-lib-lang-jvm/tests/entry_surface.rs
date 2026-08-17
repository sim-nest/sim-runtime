#[test]
fn jvm_has_exactly_one_public_drive_entry_point() {
    let source = include_str!("../src/entry.rs");
    assert_eq!(source.matches("pub fn drive<").count(), 1);
    for bypass in [
        "pub fn drive_method",
        "pub fn drive_intrinsic",
        "pub fn drive_dynamic",
    ] {
        assert!(!source.contains(bypass), "second drive surface: {bypass}");
    }
    for target in ["Method", "Intrinsic", "Dynamic"] {
        assert!(
            source.contains(target),
            "missing shared target family: {target}"
        );
    }
}
