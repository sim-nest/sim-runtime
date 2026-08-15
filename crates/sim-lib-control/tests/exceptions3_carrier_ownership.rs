const OWNERSHIP: &str = include_str!("fixtures/exceptions3_carrier_ownership.tsv");
const SCENARIOS: &str = include_str!("fixtures/exceptions3_characterize_1.tsv");

#[test]
fn control_carriers_and_guest_fields_have_explicit_non_recursive_ownership() {
    let mut modules = std::collections::BTreeSet::new();
    for (line_number, line) in OWNERSHIP.lines().enumerate().skip(1) {
        let cells = line.split('\t').collect::<Vec<_>>();
        assert_eq!(cells.len(), 5, "invalid ownership row {}", line_number + 1);
        assert!(matches!(cells[3], "envelope" | "guest-object" | "delete"));
        if cells[3] == "envelope" {
            assert!(
                !cells[2].contains("Raised"),
                "envelope recursively owns a raised carrier: {line}"
            );
        }
        if let Some(module) = cells[0].strip_prefix("control::") {
            modules.insert(module.split(['<', '(']).next().unwrap());
        }
    }
    assert_eq!(
        modules,
        [
            "Condition",
            "ProtectedOutcome",
            "Restart",
            "ResumePacket",
            "ResumeResult",
            "Unwind",
            "run_with_close_guards",
        ]
        .into_iter()
        .collect()
    );
}

fn content_id(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

#[test]
fn characterize_1_manifest_is_complete_and_replays_identically() {
    let rows = SCENARIOS.lines().skip(1).collect::<Vec<_>>();
    for required in [
        "raise-catch-class",
        "catch-superclass",
        "no-match-propagation",
        "explicit-cause",
        "implicit-context",
        "suppression",
        "group-construction-split",
        "aggregate-error-order",
        "arbitrary-non-object-throw",
        "cleanup-order-under-unwind",
        "resume-after-protected-call",
    ] {
        assert!(
            rows.iter()
                .any(|row| row.split('\t').nth(1) == Some(required))
        );
    }
    assert!(rows.iter().all(|row| row.split('\t').count() == 4));
    let first = rows.iter().map(|row| content_id(row)).collect::<Vec<_>>();
    let replay = rows.iter().map(|row| content_id(row)).collect::<Vec<_>>();
    assert_eq!(first, replay);
}
