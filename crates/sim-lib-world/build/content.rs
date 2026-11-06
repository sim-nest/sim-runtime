// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Path-package locations and content checksums for one resolver domain.

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
        if let Some(hashed) = hash_package(root, index, &package.name, &package.version) {
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
            return Classified {
                source: hashed.source,
                checksum: hashed.checksum,
                kind: "path-content",
                files: Some(hashed.files),
                observed: String::new(),
                rerun: hashed.rerun,
            };
        }
        return unverified("missing-path");
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
    Classified {
        source: package.source.clone(),
        checksum: package.checksum.clone(),
        kind,
        files: None,
        observed: String::new(),
        rerun: Vec::new(),
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

fn hash_package(root: &Path, index: &Index, name: &str, version: &str) -> Option<Hashed> {
    let source = index.lookup(name, version)?;
    let dir = if source == "." {
        root.to_path_buf()
    } else {
        root.join(&source)
    };
    let canon = dir.canonicalize().ok()?;
    if !canon.starts_with(root) {
        return None;
    }
    let mut files = Vec::new();
    let mut links = Vec::new();
    collect(&canon, &canon, &mut files, &mut links);
    files.sort();
    links.sort();
    let mut body = String::new();
    body.push_str("path\n");
    body.push_str(&source);
    body.push('\n');
    let mut rerun = Vec::new();
    for (rel, path) in &files {
        let bytes =
            fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        body.push_str(rel);
        body.push('\n');
        body.push_str(&sha256::hex(&sha256::sha256(&bytes)));
        body.push('\n');
        rerun.push(path.clone());
    }
    for (rel, target) in &links {
        body.push_str("link\n");
        body.push_str(rel);
        body.push('\n');
        body.push_str(target);
        body.push('\n');
    }
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
    let checksum = sha256::hex(&sha256::sha256(body.as_bytes()));
    if !files.is_empty() {
        let path_only = sha256::hex(&sha256::sha256(format!("path\n{source}\n").as_bytes()));
        assert_ne!(
            path_only, checksum,
            "path content hashes the files under {source}"
        );
    }
    let files = u64::try_from(files.len()).expect("file count");
    Some(Hashed {
        source,
        checksum,
        files,
        rerun,
    })
}

fn collect(
    root: &Path,
    dir: &Path,
    files: &mut Vec<(String, PathBuf)>,
    links: &mut Vec<(String, String)>,
) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|error| panic!("read {}: {error}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("dir entry: {error}"));
        let path = entry.path();
        let name = entry.file_name();
        let name = name
            .to_str()
            .unwrap_or_else(|| panic!("utf8 name in {}", path.display()));
        let file_type = entry
            .file_type()
            .unwrap_or_else(|error| panic!("file type: {error}"));
        if file_type.is_symlink() {
            let target = fs::read_link(&path).unwrap_or_else(|error| panic!("readlink: {error}"));
            let rel = relative(root, &path);
            let target = target
                .to_str()
                .unwrap_or_else(|| panic!("utf8 link {}", path.display()))
                .to_owned();
            links.push((rel, target));
            continue;
        }
        if file_type.is_dir() {
            if name == "target" || name == ".git" || name == ".sim" {
                continue;
            }
            collect(root, &path, files, links);
            continue;
        }
        if file_type.is_file() {
            files.push((relative(root, &path), path));
        }
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| {
            component
                .as_os_str()
                .to_str()
                .expect("utf8 path")
                .to_owned()
        })
        .collect::<Vec<_>>()
        .join("/")
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
