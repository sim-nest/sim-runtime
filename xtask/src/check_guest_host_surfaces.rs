//! Checks the declared guest-to-host boundary and its product anchors.

use std::{fs, path::Path};

const PRODUCT_FORBIDDEN: &[&str] = &[
    "std::env::var(",
    "std::env::vars(",
    "std::process::Command",
    "Command::new(",
    "std::fs::File",
    "libloading::",
];

pub fn run(args: Vec<String>) -> Result<(), String> {
    let program = args.first().map(String::as_str).unwrap_or("xtask");
    if args.get(1).map(String::as_str) != Some("check-guest-host-surfaces") || args.len() != 2 {
        return Err(format!("usage: {program} check-guest-host-surfaces"));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| "xtask manifest should have a workspace root parent".to_owned())?;
    let ledger = fs::read_to_string(root.join("guest-host-surfaces.toml"))
        .map_err(|error| format!("read guest-host-surfaces.toml: {error}"))?;
    for required in [
        "kind = \"product\"",
        "kind = \"tooling\"",
        "ambient_host_apis = \"forbidden\"",
        "allowed-for-build-and-proof-only",
    ] {
        if !ledger.contains(required) {
            return Err(format!("guest host-surface ledger is missing `{required}`"));
        }
    }
    for anchor in [
        "crates/sim-lib-lang-lua/src/stdlib_os.rs",
        "crates/sim-lib-lang-python/src/library_core.rs",
        "crates/sim-lib-lang-jvm/src/surface.rs",
    ] {
        let source = fs::read_to_string(root.join(anchor))
            .map_err(|error| format!("read {anchor}: {error}"))?;
        for forbidden in PRODUCT_FORBIDDEN {
            if source.contains(forbidden) {
                return Err(format!("{anchor} contains ambient host API `{forbidden}`"));
            }
        }
    }
    println!("check-guest-host-surfaces: OK");
    Ok(())
}
