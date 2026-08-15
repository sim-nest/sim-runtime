const PUBLIC_SURFACE: &str = concat!(
    include_str!("../src/lib.rs"),
    include_str!("../src/admission.rs")
);

#[test]
fn public_surface_remains_neutral() {
    let banned_vocabulary = [
        ("guest container format", concat!("Class", "File")),
        ("guest constant table", concat!("Constant", "Pool")),
        ("guest-specific runtime", concat!("J", "vm")),
        ("guest-specific language", concat!("Ja", "va")),
        ("host scheduling", concat!("Thr", "ead")),
        ("wall-time source", concat!("System", "Time")),
        ("monotonic-time source", concat!("Inst", "ant")),
        ("time quantity", concat!("Dur", "ation")),
        ("ambient file handle", concat!("std::fs::", "File")),
        ("ambient network handle", concat!("Tcp", "Stream")),
        ("owned text type", concat!("Str", "ing")),
        ("borrowed text type", concat!("&", "str")),
    ];

    for (category, needle) in banned_vocabulary {
        assert!(
            !PUBLIC_SURFACE.contains(needle),
            "neutral public surface contains banned {category}: {needle}"
        );
    }
}

#[test]
fn every_policy_trait_names_two_conceivable_consumers() {
    for trait_name in [
        "InstructionPolicy",
        "ValueWidthPolicy",
        "EffectPolicy",
        "FramePolicy",
        "HandlerPolicy",
        "RootPolicy",
        "SafepointPolicy",
        "AdmissionPolicy",
        "ReceiptPolicy",
    ] {
        let trait_at = PUBLIC_SURFACE
            .find(&format!("pub trait {trait_name}"))
            .unwrap_or_else(|| panic!("missing {trait_name}"));
        let docs = &PUBLIC_SURFACE[..trait_at];
        let paragraph = docs.rsplit("\n\n").next().expect("trait rustdoc");
        assert!(
            paragraph.contains(" or "),
            "{trait_name} rustdoc must name two conceivable consumers"
        );
    }
}
