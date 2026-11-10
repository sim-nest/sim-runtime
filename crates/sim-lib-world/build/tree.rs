// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reached bytes of one path package.
//!
//! A symlink is followed only when its canonical referent stays inside the
//! package root. The digest names that path and hashes the reached file
//! bytes. It does not record the link text. A referent outside the package
//! refuses the package.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::sha256;

#[derive(Debug)]
pub(super) enum TreeFault {
    Missing,
    SymlinkEscape(String),
}

pub(super) fn package_dir(workspace: &Path, package: &Path) -> Result<PathBuf, TreeFault> {
    let root = workspace.canonicalize().map_err(|_| TreeFault::Missing)?;
    let canon = package.canonicalize().map_err(|_| TreeFault::Missing)?;
    if !canon.starts_with(&root) {
        return Err(TreeFault::SymlinkEscape("package-root".to_owned()));
    }
    Ok(canon)
}

/// Files reached inside the package, plus the symlink paths and their parent
/// directories. Cargo follows a symlink watch to the target, so the parent
/// directory is what changes when the link is retargeted.
#[derive(Debug)]
pub(super) struct PackageTree {
    pub(super) files: Vec<(String, PathBuf)>,
    pub(super) watches: Vec<PathBuf>,
}

pub(super) fn read_tree(package_dir: &Path) -> Result<PackageTree, TreeFault> {
    let root = package_dir.canonicalize().map_err(|_| TreeFault::Missing)?;
    let mut files = Vec::new();
    let mut watches = Vec::new();
    let mut visiting = BTreeSet::new();
    walk(&root, &root, "", &mut visiting, &mut files, &mut watches)?;
    files.sort();
    watches.sort();
    watches.dedup();
    Ok(PackageTree { files, watches })
}

pub(super) fn path_preface(source: &str, files: &[(String, PathBuf)]) -> String {
    let mut body = String::new();
    body.push_str("path\n");
    body.push_str(source);
    body.push('\n');
    for (rel, path) in files {
        let bytes =
            fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        body.push_str(rel);
        body.push('\n');
        body.push_str(&sha256::hex(&sha256::sha256(&bytes)));
        body.push('\n');
    }
    body
}

pub(super) fn sha256_hex(bytes: &[u8]) -> String {
    sha256::hex(&sha256::sha256(bytes))
}

fn walk(
    package_root: &Path,
    dir: &Path,
    prefix: &str,
    visiting: &mut BTreeSet<PathBuf>,
    files: &mut Vec<(String, PathBuf)>,
    watches: &mut Vec<PathBuf>,
) -> Result<(), TreeFault> {
    let dir = dir.canonicalize().map_err(|_| TreeFault::Missing)?;
    if !dir.starts_with(package_root) {
        return Err(TreeFault::SymlinkEscape(prefix.to_owned()));
    }
    if !visiting.insert(dir.clone()) {
        return Ok(());
    }
    let result = read_entries(package_root, &dir, prefix, visiting, files, watches);
    visiting.remove(&dir);
    result
}

fn read_entries(
    package_root: &Path,
    dir: &Path,
    prefix: &str,
    visiting: &mut BTreeSet<PathBuf>,
    files: &mut Vec<(String, PathBuf)>,
    watches: &mut Vec<PathBuf>,
) -> Result<(), TreeFault> {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|error| panic!("read {}: {error}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("dir entry: {error}"));
        let path = entry.path();
        let name = entry.file_name();
        let name = name
            .to_str()
            .unwrap_or_else(|| panic!("utf8 name in {}", path.display()));
        let rel = join_rel(prefix, name);
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("file type: {error}"));
        if file_type.is_symlink() {
            watches.push(path.clone());
            if let Some(parent) = path.parent() {
                watches.push(parent.to_path_buf());
            }
            follow(&path, &rel, package_root, visiting, files, watches)?;
            continue;
        }
        if file_type.is_dir() {
            if name == "target" || name == ".git" || name == ".sim" {
                continue;
            }
            walk(package_root, &path, &rel, visiting, files, watches)?;
            continue;
        }
        if file_type.is_file() {
            files.push((rel, path));
        }
    }
    Ok(())
}

fn follow(
    path: &Path,
    rel: &str,
    package_root: &Path,
    visiting: &mut BTreeSet<PathBuf>,
    files: &mut Vec<(String, PathBuf)>,
    watches: &mut Vec<PathBuf>,
) -> Result<(), TreeFault> {
    let canon = fs::canonicalize(path).map_err(|_| TreeFault::SymlinkEscape(rel.to_owned()))?;
    if !canon.starts_with(package_root) {
        return Err(TreeFault::SymlinkEscape(rel.to_owned()));
    }
    if canon.is_dir() {
        walk(package_root, &canon, rel, visiting, files, watches)?;
    } else if canon.is_file() {
        files.push((rel.to_owned(), canon));
    }
    Ok(())
}

fn join_rel(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_owned()
    } else {
        format!("{prefix}/{name}")
    }
}
