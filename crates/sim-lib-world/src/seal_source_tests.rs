// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use sim_kernel::{Datum, Symbol};

use crate::command::render;
use crate::seal;

#[path = "../build/sha256.rs"]
mod sha256;
#[path = "../build/tree.rs"]
mod tree;

fn node<'a>(value: &'a Datum, tag: &str) -> &'a [(Symbol, Datum)] {
    match value {
        Datum::Node {
            tag: actual,
            fields,
            ..
        } if actual.as_qualified_str() == tag => fields,
        other => panic!("expected {tag}, got {other:?}"),
    }
}

fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> &'a Datum {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, value)| value)
        .unwrap_or_else(|| panic!("missing {name}"))
}

fn text(value: &Datum) -> &str {
    match value {
        Datum::String(text) => text,
        other => panic!("string, got {other:?}"),
    }
}

fn scratch(name: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/seal-source-tests")
        .join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("scratch");
    root
}

fn write(path: &Path, bytes: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("parent");
    }
    fs::write(path, bytes).expect("write");
}

fn document(member: &str) -> String {
    format!(
        "\
sim-world-inputs-v1
config lock 4
config resolver 3
root-package demo
sealed-target x86_64-unknown-linux-gnu
toolchain commit aaa
toolchain host x86_64-unknown-linux-gnu
toolchain release 1.0.0
domain resolver/demo . compiling Cargo.lock
alias resolver/demo .
{member}
"
    )
}

fn open_member(member: &str) -> Datum {
    seal::open_inputs(&document(member)).expect("inputs")
}

fn member_fields(member: &str) -> Vec<(Symbol, Datum)> {
    let universe = seal::universe_inputs(&document(member)).expect("universe");
    let rows = node(&universe, "world/source-universe-v1");
    let row = node(
        &vector(field(rows, "member-rows"))[0],
        "world/source-member-v1",
    );
    row.to_vec()
}

fn vector(value: &Datum) -> &[Datum] {
    match value {
        Datum::Vector(values) => values,
        other => panic!("vector, got {other:?}"),
    }
}

#[test]
fn reached_registry_bytes_verify_and_keep_the_semantic_key() {
    let member = "member resolver/demo registry demo-crate 1.0.0 registry+https://example abcdef registry closure abcdef";
    let sealed = open_member(member);
    let fields = node(&sealed, "world/source-seal-v1");
    assert_eq!(text(field(fields, "admission")), "admit");
    assert_eq!(field(fields, "compiler-opened"), &Datum::Bool(false));
    let row = member_fields(member);
    assert_eq!(text(field(&row, "status")), "verified");
    assert_eq!(text(field(&row, "checksum")), "abcdef");
    assert_eq!(
        text(field(&row, "content")),
        "resolver/demo registry demo-crate 1.0.0 registry+https://example"
    );
    assert_ne!(text(field(&row, "content")), text(field(&row, "checksum")));
    assert!(!render(&sealed).contains("source-not-reached"));
}

#[test]
fn unreached_registry_checksum_stays_metadata() {
    let sealed = open_member(
        "member resolver/demo registry demo-crate 1.0.0 registry+https://example abcdef registry closure",
    );
    let fields = node(&sealed, "world/source-seal-v1");
    assert_eq!(text(field(fields, "admission")), "refuse");
    assert_eq!(text(field(fields, "reason")), "source-not-reached");
    assert_eq!(field(fields, "compiler-opened"), &Datum::Bool(false));
    assert_eq!(field(fields, "effects"), &crate::query::count_datum(0));
}

#[test]
fn mismatched_reached_bytes_are_corrupt_without_a_kind_word() {
    let sealed = open_member(
        "member resolver/demo registry demo-crate 1.0.0 registry+https://example abcdef registry closure 0000",
    );
    let fields = node(&sealed, "world/source-seal-v1");
    assert_eq!(text(field(fields, "admission")), "refuse");
    assert_eq!(text(field(fields, "reason")), "corrupt-input");
    assert_eq!(field(fields, "compiler-opened"), &Datum::Bool(false));
}

#[test]
fn registry_origin_with_a_path_kind_still_needs_reached_bytes() {
    let sealed = open_member(
        "member resolver/demo registry demo-crate 1.0.0 registry+https://example abcdef path-content closure",
    );
    let fields = node(&sealed, "world/source-seal-v1");
    assert_eq!(text(field(fields, "admission")), "refuse");
    assert_eq!(text(field(fields, "reason")), "source-not-reached");
    let git = open_member(
        "member resolver/demo git demo-crate 1.0.0 git+https://example abcdef path-content closure",
    );
    let fields = node(&git, "world/source-seal-v1");
    assert_eq!(text(field(fields, "reason")), "source-not-reached");
}

#[test]
fn symlink_escape_refuses_before_compile() {
    let sealed = open_member(
        "member resolver/demo path demo-crate 1.0.0 crates/demo - symlink-escape closure src/out.rs",
    );
    let fields = node(&sealed, "world/source-seal-v1");
    assert_eq!(text(field(fields, "admission")), "refuse");
    assert_eq!(text(field(fields, "reason")), "symlink-escape");
    assert_eq!(field(fields, "compiler-opened"), &Datum::Bool(false));
}

#[test]
fn removed_and_corrupted_bytes_change_the_digest() {
    let root = scratch("bytes");
    let pkg = root.join("pkg");
    write(&pkg.join("src/lib.rs"), "fn demo() {}\n");
    write(&pkg.join("Cargo.toml"), "[package]\nname = \"demo\"\n");
    let first = digest(&pkg).expect("first");
    write(&pkg.join("src/lib.rs"), "fn demo() { let _ = 1; }\n");
    let corrupted = digest(&pkg).expect("corrupted");
    assert_ne!(first, corrupted);
    fs::remove_file(pkg.join("src/lib.rs")).expect("remove");
    let removed = digest(&pkg).expect("removed");
    assert_ne!(corrupted, removed);
}

#[test]
fn an_in_root_symlink_hashes_reached_bytes_not_the_link_text() {
    let root = scratch("link");
    let pkg = root.join("pkg");
    write(&pkg.join("inner.rs"), "reached\n");
    symlink("inner.rs", pkg.join("alias.rs")).expect("link");
    let first = digest(&pkg).expect("linked");
    let body = tree::path_preface("pkg", &tree::read_tree(&pkg).expect("files").files);
    assert!(body.contains("alias.rs\n"), "{body}");
    assert!(body.contains("inner.rs\n"), "{body}");
    assert!(!body.contains("../"));
    write(&pkg.join("inner.rs"), "changed\n");
    let changed = digest(&pkg).expect("changed referent");
    assert_ne!(first, changed);
    fs::remove_file(pkg.join("alias.rs")).expect("unlink");
    symlink("../pkg/inner.rs", pkg.join("alias.rs")).expect("relink");
    let relinked = digest(&pkg).expect("same bytes");
    assert_eq!(changed, relinked);
    let body = tree::path_preface("pkg", &tree::read_tree(&pkg).expect("files").files);
    assert!(!body.contains("../pkg/inner.rs"), "{body}");
}

#[test]
fn a_retargeted_directory_symlink_changes_the_digest() {
    let root = scratch("dir-link");
    let pkg = root.join("pkg");
    write(&pkg.join("a/file.rs"), "alpha\n");
    write(&pkg.join("b/file.rs"), "beta\n");
    symlink("a", pkg.join("selected")).expect("link a");
    let via_a = digest(&pkg).expect("via a");
    let watches = tree::read_tree(&pkg).expect("watches").watches;
    let parent = pkg.canonicalize().expect("pkg");
    assert!(watches.contains(&parent.join("selected")), "{watches:?}");
    assert!(watches.contains(&parent), "{watches:?}");
    let linked = tree::path_preface("pkg", &tree::read_tree(&pkg).expect("files").files);
    assert!(linked.contains("a/file.rs\n"), "{linked}");
    assert!(linked.contains("selected/file.rs\n"), "{linked}");
    assert!(linked.contains(&tree::sha256_hex(b"alpha\n")), "{linked}");
    fs::remove_file(pkg.join("selected")).expect("unlink");
    symlink("b", pkg.join("selected")).expect("link b");
    let via_b = digest(&pkg).expect("via b");
    assert_ne!(via_a, via_b);
    let retargeted = tree::path_preface("pkg", &tree::read_tree(&pkg).expect("files").files);
    assert!(
        retargeted.contains(&tree::sha256_hex(b"beta\n")),
        "{retargeted}"
    );
    assert!(retargeted.contains("b/file.rs\n"), "{retargeted}");
    assert!(retargeted.contains("selected/file.rs\n"), "{retargeted}");
    write(&pkg.join("src/lib.rs"), "fn demo() {}\n");
    symlink("..", pkg.join("src/loop")).expect("cycle");
    let files = tree::read_tree(&pkg).expect("cycle stays finite").files;
    assert!(files.iter().any(|(rel, _)| rel == "src/lib.rs"));
}

#[test]
fn a_symlink_outside_the_package_refuses() {
    let root = scratch("escape");
    let pkg = root.join("pkg");
    let outside = root.join("outside.rs");
    write(&pkg.join("src/lib.rs"), "inside\n");
    write(&outside, "outside\n");
    symlink(&outside, pkg.join("src/escape.rs")).expect("escape");
    let error = digest(&pkg).expect_err("escape refuses");
    assert!(
        matches!(error, tree::TreeFault::SymlinkEscape(ref path) if path.ends_with("escape.rs")),
        "{error:?}"
    );
    write(&outside, "mutated outside\n");
    let again = digest(&pkg).expect_err("still refuses");
    assert!(matches!(again, tree::TreeFault::SymlinkEscape(_)));
}

#[test]
fn a_non_canonical_package_root_still_hashes() {
    let root = scratch("canon");
    let workspace = root.join("ws");
    let pkg = workspace.join("pkg");
    write(&pkg.join("src/lib.rs"), "canon\n");
    let via = root.join("ws/./pkg");
    let direct = digest(&pkg).expect("direct");
    let indirect = digest(&via).expect("indirect");
    assert_eq!(direct, indirect);
    let resolved = tree::package_dir(&root.join("ws/../ws"), &via).expect("inside");
    assert_eq!(resolved, pkg.canonicalize().expect("pkg"));
    symlink(outside_dir(&root), workspace.join("gone")).expect("package link");
    let escaped = tree::package_dir(&workspace, &workspace.join("gone"));
    assert!(matches!(
        escaped,
        Err(tree::TreeFault::SymlinkEscape(path)) if path == "package-root"
    ));
}

fn digest(dir: &Path) -> Result<String, tree::TreeFault> {
    let files = tree::read_tree(dir)?.files;
    Ok(tree::sha256_hex(
        tree::path_preface("pkg", &files).as_bytes(),
    ))
}

fn outside_dir(root: &Path) -> PathBuf {
    let dir = root.join("elsewhere");
    fs::create_dir_all(&dir).expect("elsewhere");
    dir
}
