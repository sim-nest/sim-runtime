//! Source-fact guard for the product JVM preparation and drive architecture.

#[test]
fn optimized_preparation_has_one_representation_one_drive_and_no_mode_switch() {
    let code = include_str!("../src/code.rs");
    let entry = include_str!("../src/entry.rs");
    let machine_driver = include_str!("../../sim-lib-machine/src/driver.rs");

    assert_eq!(
        code.matches("pub struct PreparedJvmPolicy").count(),
        1,
        "JVM preparation must retain one prepared representation family"
    );
    assert_eq!(
        entry.matches("pub fn drive<").count(),
        1,
        "JVM effects must retain one public drive entry"
    );
    assert_eq!(
        machine_driver
            .matches("pub fn drive_with_safepoints<")
            .count(),
        1,
        "prepared JVM work must retain the shared machine drive loop"
    );
    assert!(
        code.contains("PreparedMicroOp::Checked"),
        "the optimized representation must retain its ordinary checked fallback"
    );

    let lower = code.to_ascii_lowercase();
    for forbidden in [
        "pub enabled: bool",
        "force checked",
        "disable specialization",
        "experimental mode",
    ] {
        assert!(
            !lower.contains(forbidden),
            "user-selectable or experimental preparation mode remains: {forbidden}"
        );
    }
}
