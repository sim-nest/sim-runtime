// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded compile case for one admitted source seal.
//!
//! A missing input, a corrupt input, a wrong target, and a stub-only
//! input refuse before a compiler starts. One well-formed admitted
//! input then runs one offline locked check. Equal inputs keep one
//! seal. A changed lock moves that seal. Timing a captured string stays
//! a measurement and opens no compiler.

use std::process::Command;
use std::sync::Mutex;

use sim_kernel::{Datum, Symbol};

use crate::query;
use crate::seal;

const MISSING: &str = "\
sim-world-inputs-v1
config lock 4
config resolver 3
root-package demo
sealed-target x86_64-unknown-linux-gnu
toolchain commit 0
toolchain host x86_64-unknown-linux-gnu
toolchain release 0
domain resolver/demo . compiling Cargo.lock
alias resolver/demo .
member resolver/demo registry demo-crate 1.0.0 registry+https://github.com/rust-lang/crates.io-index - none closure
";

const CORRUPT: &str = "\
sim-world-inputs-v1
config lock 4
config resolver 3
root-package demo
sealed-target x86_64-unknown-linux-gnu
toolchain commit 0
toolchain host x86_64-unknown-linux-gnu
toolchain release 0
domain resolver/demo . compiling Cargo.lock
alias resolver/demo .
member resolver/demo registry demo-crate 1.0.0 registry+https://github.com/rust-lang/crates.io-index aaaa corrupt bbbb closure
";

const STUB: &str = "\
sim-world-inputs-v1
config lock 4
config resolver 3
root-package demo
sealed-target x86_64-unknown-linux-gnu
toolchain commit 0
toolchain host x86_64-unknown-linux-gnu
toolchain release 0
domain resolver/demo . compiling Cargo.lock
alias resolver/demo .
member resolver/demo stub demo-crate 1.0.0 stub+metadata metadata-only none closure
";

const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

struct Face {
    admission: String,
    reason: String,
    digest: String,
    compiler_opened: bool,
    sealed_target: String,
}

/// Runs the loaded measure command.
///
/// The first call in this process performs the check. Later calls return
/// that same record, including a refusal, so a sampled status does not
/// start another compiler.
///
/// # Errors
/// Returns an error when the record cannot be represented.
pub(crate) fn run() -> Result<Datum, crate::WorldError> {
    static CACHED: Mutex<Option<Datum>> = Mutex::new(None);
    let mut slot = CACHED.lock().unwrap_or_else(|poison| poison.into_inner());
    if let Some(found) = slot.clone() {
        return Ok(found);
    }
    let datum = survey();
    *slot = Some(datum.clone());
    Ok(datum)
}

fn survey() -> Datum {
    if let Err(found) = refusal(Some(MISSING), None, "missing-input") {
        return found;
    }
    if let Err(found) = refusal(Some(CORRUPT), None, "corrupt-input") {
        return found;
    }
    if let Err(found) = refusal(Some(STUB), None, "stub-only") {
        return found;
    }
    let Ok(admitted) = open_face(None, None) else {
        return gap("inconsistent-input", 0);
    };
    if admitted.compiler_opened {
        return gap("compiler-opened", 0);
    }
    let Ok(again) = open_face(None, None) else {
        return gap("inconsistent-input", 0);
    };
    if again.compiler_opened {
        return gap("compiler-opened", 0);
    }
    if admitted.digest.is_empty() || admitted.digest != again.digest {
        return gap("seal-moved", 0);
    }
    let other = other_target(&admitted.sealed_target);
    let wrong = match refusal(None, Some(other), "wrong-target") {
        Ok(face) => face,
        Err(found) => return found,
    };
    if wrong.digest != admitted.digest {
        return gap("seal-moved", 0);
    }
    let Some(flipped) = changed_lock(seal::captured_inputs()) else {
        return gap("no-registry-checksum", 0);
    };
    let Ok(moved) = open_face(Some(&flipped), None) else {
        return gap("inconsistent-input", 0);
    };
    if moved.compiler_opened {
        return gap("compiler-opened", 0);
    }
    if moved.digest == admitted.digest {
        return gap("seal-same", 0);
    }
    if admitted.admission != "admit" {
        let reason = if admitted.reason.is_empty() {
            "refused"
        } else {
            admitted.reason.as_str()
        };
        return gap(&shown(reason), 0);
    }
    match spawn_check() {
        Ok(0) => guard(record()),
        Ok(code) => gap(&shown(&format!("compiler-failed {code}")), 1),
        Err(()) => gap("compiler-failed", 0),
    }
}

fn refusal(input: Option<&str>, target: Option<&str>, reason: &str) -> Result<Face, Datum> {
    let face = open_face(input, target).map_err(|word| gap(word, 0))?;
    if face.compiler_opened {
        return Err(gap("compiler-opened", 0));
    }
    if face.reason != reason {
        return Err(gap(&shown(&face.reason), 0));
    }
    Ok(face)
}

fn open_face(input: Option<&str>, target: Option<&str>) -> Result<Face, &'static str> {
    let datum = seal::open_admitted(input, target).map_err(|_| "inconsistent-input")?;
    let Datum::Node { tag, fields } = &datum else {
        return Err("inconsistent-input");
    };
    if tag.as_qualified_str() == "world/source-gap-v1" {
        return Err("unknown-batch");
    }
    if tag.as_qualified_str() != "world/source-seal-v1" {
        return Err("inconsistent-input");
    }
    Ok(Face {
        admission: string_field(fields, "admission"),
        reason: string_field(fields, "reason"),
        digest: string_field(fields, "digest"),
        compiler_opened: bool_field(fields, "compiler-opened"),
        sealed_target: string_field(fields, "sealed-target"),
    })
}

fn other_target(sealed: &str) -> &'static str {
    if sealed == "wasm32-unknown-unknown" {
        "x86_64-unknown-linux-gnu"
    } else {
        "wasm32-unknown-unknown"
    }
}

fn changed_lock(text: &str) -> Option<String> {
    let mut flipped = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        if !flipped && let Some(updated) = flip_registry_checksum(line) {
            lines.push(updated);
            flipped = true;
        } else {
            lines.push(line.to_owned());
        }
    }
    if flipped {
        Some(lines.join("\n"))
    } else {
        None
    }
}

fn flip_registry_checksum(line: &str) -> Option<String> {
    let parts: Vec<&str> = line.split(' ').collect();
    if parts.first() != Some(&"member")
        || parts.len() < 9
        || parts[7] != "registry"
        || parts[6] == "-"
    {
        return None;
    }
    let mut checksum = parts[6].to_owned();
    let last = checksum.pop()?;
    checksum.push(if last == 'a' { 'b' } else { 'a' });
    let mut owned: Vec<String> = parts.into_iter().map(str::to_owned).collect();
    owned[6] = checksum;
    Some(owned.join(" "))
}

/// The nested check uses its own target directory. The caller may already
/// be a cargo process holding the workspace target lock, and a second
/// cargo on that same directory would wait forever. The directory stays
/// under the workspace `target` tree.
fn spawn_check() -> Result<i32, ()> {
    let root = seal::workspace_root();
    let target = root.join("target").join("seal-measure");
    let mut command = Command::new("cargo");
    command
        .args([
            "check",
            "-p",
            "sim-lib-world",
            "--locked",
            "--offline",
            "--quiet",
        ])
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", &target);
    match command.status() {
        Ok(status) => Ok(status.code().unwrap_or(1)),
        Err(_) => Err(()),
    }
}

fn record() -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "seal-measure-v1"),
        fields: vec![
            (Symbol::new("compile"), string("checked")),
            (Symbol::new("corrupt"), string("corrupt-input")),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("lock"), string("moved")),
            (Symbol::new("missing"), string("missing-input")),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("seal"), string("same")),
            (Symbol::new("stub"), string("stub-only")),
            (Symbol::new("subprocesses"), query::count_datum(1)),
            (Symbol::new("target"), string("wrong-target")),
        ],
    }
}

fn gap(reason: &str, subprocesses: u64) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "seal-measure-gap-v1"),
        fields: vec![
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("reason"), string(shown(reason))),
            (Symbol::new("resident"), Datum::Bool(true)),
            (
                Symbol::new("subprocesses"),
                query::count_datum(subprocesses),
            ),
        ],
    }
}

fn guard(datum: Datum) -> Datum {
    if leaked_datum(&datum) {
        gap("private-leakage", 0)
    } else {
        datum
    }
}

fn leaked_datum(value: &Datum) -> bool {
    match value {
        Datum::String(text) => leaked(text),
        Datum::Node { fields, .. } => fields.iter().any(|(_, field)| leaked_datum(field)),
        Datum::Vector(values) => values.iter().any(leaked_datum),
        _ => false,
    }
}

fn leaked(value: &str) -> bool {
    MARKERS.iter().any(|marker| value.contains(marker))
}

fn shown(value: &str) -> String {
    if leaked(value) {
        String::new()
    } else {
        value.to_owned()
    }
}

fn string(value: impl Into<String>) -> Datum {
    Datum::String(value.into())
}

fn string_field(fields: &[(Symbol, Datum)], name: &str) -> String {
    match field(fields, name) {
        Some(Datum::String(text)) => text.clone(),
        _ => String::new(),
    }
}

fn bool_field(fields: &[(Symbol, Datum)], name: &str) -> bool {
    matches!(field(fields, name), Some(Datum::Bool(true)))
}

fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Option<&'a Datum> {
    fields
        .iter()
        .find(|(field, _)| field.to_string() == name)
        .map(|(_, value)| value)
}
