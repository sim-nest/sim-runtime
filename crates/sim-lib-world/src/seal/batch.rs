// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Sealed input text for the owning workspace and the model batches.

#[cfg(test)]
use crate::WorldError;

const OWNING: &str = include_str!(concat!(env!("OUT_DIR"), "/owning-inputs.txt"));

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
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

#[cfg(test)]
const ABSENT: &str = "\
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
member resolver/demo absent demo-crate 1.0.0 absent - none closure
";

#[cfg(test)]
const WRONG_TARGET: &str = "\
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
member resolver/demo registry demo-crate 1.0.0 registry+https://github.com/rust-lang/crates.io-index abcdef0123456789 registry closure
";

#[cfg(test)]
const NESTED: &str = "\
sim-world-inputs-v1
config lock 4
config resolver 3
root-package parent
sealed-target x86_64-unknown-linux-gnu
toolchain commit 0
toolchain host x86_64-unknown-linux-gnu
toolchain release 0
domain resolver/parent . compiling Cargo.lock
alias resolver/parent .
domain resolver/parent/vendor/other vendor/other enumerated vendor/other/Cargo.lock
alias resolver/parent/vendor/other vendor/other
member resolver/parent path parent 0.1.0 crates/parent abcdef path-content closure
member resolver/parent/vendor/other registry other 1.0.0 registry+https://github.com/rust-lang/crates.io-index - none closure
";

#[cfg(test)]
const GIT_MISSING: &str = "\
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
member resolver/demo git demo-crate 1.0.0 git+https://github.com/example/demo - none closure
";

#[cfg(test)]
const VERSION_MISMATCH: &str = "\
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
member resolver/demo path demo-crate 1.0.0 crates/demo abc path-content closure
member resolver/demo path stale-crate 0.1.0 crates/stale def version-mismatch outside 0.2.0
deps resolver/demo demo-crate 1.0.0 crates/demo
deps resolver/demo stale-crate 0.1.0 crates/stale
";

#[cfg(test)]
const ALIAS: &str = "\
sim-world-inputs-v1
config lock 4
config resolver 3
root-package parent
sealed-target x86_64-unknown-linux-gnu
toolchain commit 0
toolchain host x86_64-unknown-linux-gnu
toolchain release 0
spell /work/sim-runtime compiling Cargo.lock
spell /work/sim-runtime/ compiling Cargo.lock
spell /work/sim-runtime/. compiling Cargo.lock
spell /work/sim-runtime/nested compiling nested/Cargo.lock
member /work/sim-runtime path parent 0.1.0 crates/parent abcdef path-content closure
member /work/sim-runtime/nested path nested-crate 0.1.0 crates/nested 123456 path-content closure
";

pub(super) const LOCAL_BATCHES: &[&str] = &[
    "owning",
    "changed",
    "missing",
    "corrupt",
    "stub",
    "absent",
    "wrong-target",
    "nested",
    "alias",
    "git-missing",
    "version-mismatch",
    "precise",
    "precise-toolchain",
    "precise-feature",
    "precise-generated",
    "precise-edge",
    "precise-closure",
    "precise-role",
];

pub(super) fn is_local_batch(name: &str) -> bool {
    LOCAL_BATCHES.contains(&name)
}

pub(super) fn captured() -> &'static str {
    OWNING
}

pub(super) fn document_text(input: &str) -> Option<String> {
    if input.starts_with("sim-world-inputs-v1") {
        Some(input.to_owned())
    } else {
        None
    }
}

#[cfg(test)]
pub(super) fn batch_text(name: &str) -> Result<Option<String>, WorldError> {
    if let Some(text) = document_text(name) {
        return Ok(Some(text));
    }
    // Named examples stay available to this crate's tests. The loaded
    // command calls `document_text` and never this table.
    named_example(name)
}

#[cfg(test)]
fn named_example(name: &str) -> Result<Option<String>, WorldError> {
    if let Some(text) = crate::fixture::seal_text(name) {
        return Ok(Some(text.to_owned()));
    }
    if !is_local_batch(name) {
        return Ok(None);
    }
    let text = match name {
        "owning" => OWNING.to_owned(),
        "changed" => changed_owning()?,
        "missing" => MISSING.to_owned(),
        "corrupt" => CORRUPT.to_owned(),
        "stub" => STUB.to_owned(),
        "absent" => ABSENT.to_owned(),
        "wrong-target" => WRONG_TARGET.to_owned(),
        "nested" => NESTED.to_owned(),
        "alias" => ALIAS.to_owned(),
        "git-missing" => GIT_MISSING.to_owned(),
        "version-mismatch" => VERSION_MISMATCH.to_owned(),
        "precise" => precise(""),
        "precise-toolchain" => precise("toolchain"),
        "precise-feature" => precise("feature"),
        "precise-generated" => precise("generated"),
        "precise-edge" => precise("edge"),
        "precise-closure" => precise("closure"),
        "precise-role" => precise("role"),
        _ => {
            return Err(WorldError::Qualification(format!(
                "unrendered seal batch {name}"
            )));
        }
    };
    Ok(Some(text))
}

#[cfg(test)]
fn precise(edit: &str) -> String {
    let release = if edit == "toolchain" {
        "1.0.1"
    } else {
        "1.0.0"
    };
    let body = if edit == "feature" {
        "[\"dep:other\"]"
    } else {
        "[\"dep:demo-codec\"]"
    };
    let generated = if edit == "generated" { "bbbb" } else { "aaaa" };
    let dep = if edit == "edge" {
        "demo-dep~2.0.0"
    } else {
        "demo-dep~1.0.0"
    };
    let closure = if edit == "closure" {
        "outside"
    } else {
        "closure"
    };
    let role = if edit == "role" {
        "enumerated"
    } else {
        "compiling"
    };
    format!(
        "\
sim-world-inputs-v1
config lock 4
config resolver 3
root-package demo
sealed-target x86_64-unknown-linux-gnu
toolchain commit aaa
toolchain host x86_64-unknown-linux-gnu
toolchain release {release}
feature demo native-export {body}
domain resolver/demo . {role} Cargo.lock
alias resolver/demo .
member resolver/demo registry demo-crate 1.0.0 registry+https://github.com/rust-lang/crates.io-index abcdef registry {closure}
deps resolver/demo demo-crate 1.0.0 registry+https://github.com/rust-lang/crates.io-index {dep}
generated resolver/demo src/generated.rs 4 {generated}
"
    )
}

#[cfg(test)]
fn changed_owning() -> Result<String, WorldError> {
    let mut flipped = false;
    let mut lines = Vec::new();
    for line in OWNING.lines() {
        if !flipped && let Some(updated) = flip_registry_checksum(line) {
            lines.push(updated);
            flipped = true;
        } else {
            lines.push(line.to_owned());
        }
    }
    if !flipped {
        return Err(WorldError::Qualification(
            "owning lock has no registry checksum".to_owned(),
        ));
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
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

#[cfg(test)]
mod tests {
    use super::LOCAL_BATCHES;
    use super::batch_text;

    #[test]
    fn every_local_batch_has_text() {
        for name in LOCAL_BATCHES {
            let text = batch_text(name).expect("batch");
            assert!(text.is_some(), "{name}");
        }
    }
}
