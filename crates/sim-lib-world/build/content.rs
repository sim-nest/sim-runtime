// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Path-package locations, reached registry crate bytes, and content checksums.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::Pkg;
use super::parse;
use super::sha256;

pub(super) struct Classified {
    pub(super) source: String,
    pub(super) checksum: String,
    pub(super) kind: &'static str,
    pub(super) files: Option<u64>,
    pub(super) observed: String,
    pub(super) rerun: Vec<PathBuf>,
}

pub(super) struct Index {
    exact: BTreeMap<(String, String), String>,
    names: BTreeMap<String, BTreeSet<String>>,
}

impl Index {
    fn lookup(&self, name: &str, version: &str) -> Option<String> {
        if let Some(found) = self.exact.get(&(name.to_owned(), version.to_owned())) {
            return Some(found.clone());
        }
        let paths = self.names.get(name)?;
        if paths.len() == 1 {
            paths.iter().next().cloned()
        } else {
            None
        }
    }

    fn record(&mut self, name: String, version: String, rel: String) {
        let key = (name.clone(), version);
        match self.exact.get(&key) {
            Some(existing) if existing != &rel => {
                self.exact.remove(&key);
            }
            Some(_) => {}
            None => {
                self.exact.insert(key, rel.clone());
            }
        }
        self.names.entry(name).or_default().insert(rel);
    }
}

pub(super) fn domain_index(root: &Path, domain_dir: &Path) -> Index {
    index_for(root, domain_dir)
}

pub(super) fn classify(root: &Path, index: &Index, package: &Pkg, origin: &str) -> Classified {
    if origin == "path" {
        return classify_path(root, index, package);
    }
    if package.checksum.is_empty() {
        let source = if package.source.is_empty() {
            "missing-path"
        } else {
            package.source.as_str()
        };
        return unverified(source);
    }
    let kind = if origin == "git" { "git" } else { "registry" };
    let mut observed = String::new();
    let mut rerun = Vec::new();
    if kind == "registry"
        && let Some(reached) = observe_registry(package)
    {
        observed = reached.hash;
        rerun.push(reached.path);
    }
    Classified {
        source: package.source.clone(),
        checksum: package.checksum.clone(),
        kind,
        files: None,
        observed,
        rerun,
    }
}

fn classify_path(root: &Path, index: &Index, package: &Pkg) -> Classified {
    let hashed = match hash_package(root, index, &package.name, &package.version) {
        Ok(hashed) => hashed,
        Err(HashFault::Missing) => return unverified("missing-path"),
        Err(HashFault::Escape { source, path }) => {
            return Classified {
                source,
                checksum: "-".to_owned(),
                kind: "symlink-escape",
                files: None,
                observed: path,
                rerun: Vec::new(),
            };
        }
    };
    let manifest = if hashed.source == "." {
        root.join("Cargo.toml")
    } else {
        root.join(&hashed.source).join("Cargo.toml")
    };
    let declared = fs::read_to_string(&manifest)
        .ok()
        .and_then(|text| parse::package_identity(&text))
        .map(|(_, version)| version);
    // The directory is present, but the lock version is not the
    // version the package manifest declares.
    if declared.as_deref() != Some(package.version.as_str()) {
        return Classified {
            source: hashed.source,
            checksum: hashed.checksum,
            kind: "version-mismatch",
            files: Some(hashed.files),
            observed: declared.unwrap_or_else(|| "missing-manifest-version".to_owned()),
            rerun: hashed.rerun,
        };
    }
    Classified {
        source: hashed.source,
        checksum: hashed.checksum,
        kind: "path-content",
        files: Some(hashed.files),
        observed: String::new(),
        rerun: hashed.rerun,
    }
}

fn unverified(source: impl Into<String>) -> Classified {
    Classified {
        source: source.into(),
        checksum: "-".to_owned(),
        kind: "none",
        files: None,
        observed: String::new(),
        rerun: Vec::new(),
    }
}

struct Hashed {
    source: String,
    checksum: String,
    files: u64,
    rerun: Vec<PathBuf>,
}

enum HashFault {
    Missing,
    Escape { source: String, path: String },
}

struct Reached {
    hash: String,
    path: PathBuf,
}

fn observe_registry(package: &Pkg) -> Option<Reached> {
    let home = cargo_home()?;
    let cache = home.join("registry").join("cache");
    let file_name = format!("{}-{}.crate", package.name, package.version);
    let mut fallback = None;
    let entries = fs::read_dir(&cache).ok()?;
    for entry in entries.flatten() {
        let path = entry.path().join(&file_name);
        if !path.is_file() {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let hash = sha256::hex(&sha256::sha256(&bytes));
        if hash == package.checksum {
            return Some(Reached { hash, path });
        }
        fallback = Some(Reached { hash, path });
    }
    fallback
}

fn cargo_home() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("CARGO_HOME") {
        return Some(PathBuf::from(value));
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".cargo"))
}

fn hash_package(
    root: &Path,
    index: &Index,
    name: &str,
    version: &str,
) -> Result<Hashed, HashFault> {
    let Some(source) = index.lookup(name, version) else {
        return Err(HashFault::Missing);
    };
    let dir = if source == "." {
        root.to_path_buf()
    } else {
        root.join(&source)
    };
    let canon = match super::tree::package_dir(root, &dir) {
        Ok(canon) => canon,
        Err(super::tree::TreeFault::Missing) => return Err(HashFault::Missing),
        Err(super::tree::TreeFault::SymlinkEscape(_)) => {
            return Err(HashFault::Escape {
                source,
                path: "package-root".to_owned(),
            });
        }
    };
    let tree = match super::tree::read_tree(&canon) {
        Ok(tree) => tree,
        Err(super::tree::TreeFault::Missing) => return Err(HashFault::Missing),
        Err(super::tree::TreeFault::SymlinkEscape(path)) => {
            return Err(HashFault::Escape { source, path });
        }
    };
    let mut rerun = tree
        .files
        .iter()
        .map(|(_, path)| path.clone())
        .collect::<Vec<_>>();
    rerun.extend(tree.watches);
    let mut body = super::tree::path_preface(&source, &tree.files);
    if let Ok(text) = fs::read_to_string(canon.join("Cargo.toml")) {
        let mut decls = parse::path_declarations(&text);
        decls.sort();
        decls.dedup();
        for decl in decls {
            body.push_str("path-dep\n");
            body.push_str(&decl);
            body.push('\n');
        }
    }
    let checksum = super::tree::sha256_hex(body.as_bytes());
    if !tree.files.is_empty() {
        let path_only = super::tree::sha256_hex(format!("path\n{source}\n").as_bytes());
        assert_ne!(
            path_only, checksum,
            "path content hashes the files under {source}"
        );
    }
    let files = u64::try_from(tree.files.len()).expect("file count");
    Ok(Hashed {
        source,
        checksum,
        files,
        rerun,
    })
}

fn index_for(root: &Path, domain_dir: &Path) -> Index {
    let mut index = Index {
        exact: BTreeMap::new(),
        names: BTreeMap::new(),
    };
    let mut queue = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(rel) = relative_inside(root, domain_dir) {
        queue.push(rel);
    }
    if let Ok(text) = fs::read_to_string(domain_dir.join("Cargo.toml")) {
        for member in parse::workspace_members(&text) {
            if let Some(rel) = relative_inside(root, &domain_dir.join(member)) {
                queue.push(rel);
            }
        }
    }
    while let Some(rel) = queue.pop() {
        if !seen.insert(rel.clone()) {
            continue;
        }
        let dir = if rel == "." {
            root.to_path_buf()
        } else {
            root.join(&rel)
        };
        let Ok(text) = fs::read_to_string(dir.join("Cargo.toml")) else {
            continue;
        };
        if let Some((name, version)) = parse::package_identity(&text) {
            index.record(name, version, rel.clone());
        }
        for decl in parse::path_declarations(&text) {
            if let Some(next) = relative_inside(root, &dir.join(decl)) {
                queue.push(next);
            }
        }
    }
    index
}

fn relative_inside(root: &Path, path: &Path) -> Option<String> {
    let canon = path.canonicalize().ok()?;
    let root = root.canonicalize().ok()?;
    let rel = canon.strip_prefix(&root).ok()?;
    if rel.as_os_str().is_empty() {
        return Some(".".to_owned());
    }
    let text = rel.to_str()?.replace('\\', "/");
    if text.split('/').any(|part| part == "..") {
        None
    } else {
        Some(text)
    }
}

pub(super) fn encode_dep(spec: &str) -> String {
    assert!(
        !spec.contains('~') && !spec.contains('\n'),
        "dependency spec cannot be sealed: {spec}"
    );
    spec.replace(' ', "~")
}
