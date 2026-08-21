#[test]
fn jvm_has_exactly_one_public_drive_entry_point() {
    let source = concat!(
        include_str!("../src/entry.rs"),
        include_str!("../src/driver.rs")
    );
    assert_eq!(source.matches("pub(crate) fn drive_i32<").count(), 1);
    assert!(
        !source.contains("impl FnOnce()"),
        "drive must not accept an arbitrary effect closure"
    );
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
