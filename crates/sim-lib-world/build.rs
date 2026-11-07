// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Enumerate the workspace, or the extracted package, that compiles this crate.
//!
//! The loaded command seals the emitted input text offline. This build script
//! is the compiler's enumeration of that workspace: locks, member manifests,
//! nested Cargo locks, generated sources, features, and the toolchain that is
//! compiling the crate. It does not run when the loaded command answers.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "build/content.rs"]
mod content;
#[path = "build/parse.rs"]
mod parse;
#[path = "build/sha256.rs"]
mod sha256;

const ROOT_PACKAGE: &str = "sim-lib-world";

fn main() {
    let empty = sha256::sha256(b"");
    let abc = sha256::sha256(b"abc");
    assert_eq!(
        sha256::hex(&empty),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        "sha256 empty"
    );
    assert_eq!(
        sha256::hex(&abc),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        "sha256 abc"
    );

    let manifest_dir = PathBuf::from(env_var("CARGO_MANIFEST_DIR"));
    let compiling = parse::compiling_root(&manifest_dir);
    let root = compiling.root;
    let workspace_toml = compiling.manifest;
    let resolver = compiling.resolver;
    let members = compiling.members;
    assert!(!members.is_empty(), "workspace members");
    assert!(
        members
            .iter()
            .any(|member| member == "crates/sim-lib-world" || member == "."),
        "sim-lib-world is the compiling package"
    );

    let mut locks = Vec::new();
    collect_named(&root, "Cargo.lock", &mut locks);
    assert!(locks.iter().any(|path| path == &root.join("Cargo.lock")));

    let mut generated = Vec::new();
    collect_generated(&root, &mut generated);

    let root_name = root
        .file_name()
        .and_then(|name| name.to_str())
        .expect("workspace directory name")
        .to_owned();
    let mut domains: Vec<Domain> = Vec::new();
    let mut member_lines = Vec::new();
    let mut dep_lines = Vec::new();
    let mut path_file_lines = Vec::new();
    for lock in &locks {
        let text = fs::read_to_string(lock).expect("read lock");
        let parent = lock
            .parent()
            .expect("lock parent")
            .canonicalize()
            .expect("lock dir");
        assert!(
            parent.starts_with(&root),
            "lock outside workspace: {}",
            lock.display()
        );
        let canonical = relative(&root, &parent);
        let canonical = if canonical.is_empty() {
            ".".to_owned()
        } else {
            canonical
        };
        let id = domain_id(&root_name, &canonical);
        let role = if canonical == "." {
            "compiling"
        } else {
            "enumerated"
        };
        let lock_rel = relative(&root, lock);
        if let Some(existing) = domains.iter_mut().find(|domain| domain.id == id) {
            existing.aliases.push(canonical.clone());
            continue;
        }
        domains.push(Domain {
            id: id.clone(),
            canonical: canonical.clone(),
            role: role.to_owned(),
            lock: lock_rel,
            aliases: vec![canonical],
        });
        let (_lock_version, packages) = parse::parse_lock(&text);
        let closure = closure_indexes(
            &packages,
            if role == "compiling" {
                ROOT_PACKAGE
            } else {
                ""
            },
        );
        let index = content::domain_index(&root, &parent);
        for (index_in_lock, package) in packages.iter().enumerate() {
            let origin = origin_of(&package.source);
            let classified = content::classify(&root, &index, package, origin);
            for path in classified.rerun {
                println!("cargo:rerun-if-changed={}", path.display());
            }
            let closure_flag = if closure.contains(&index_in_lock) {
                "closure"
            } else {
                "outside"
            };
            assert!(
                !classified.source.contains(' ') && !classified.checksum.contains(' '),
                "sealed field contains a space: {} {}",
                package.name,
                classified.source
            );
            let mut member_line = format!(
                "member {id} {origin} {} {} {} {} {} {closure_flag}",
                package.name,
                package.version,
                classified.source,
                classified.checksum,
                classified.kind
            );
            if !classified.observed.is_empty() {
                assert!(
                    !classified.observed.contains(' '),
                    "observed identity contains a space"
                );
                member_line.push(' ');
                member_line.push_str(&classified.observed);
            }
            member_lines.push(member_line);
            if let Some(files) = classified.files {
                path_file_lines.push(format!("path-files {id} {} {files}", classified.source));
            }
            let mut specs = package
                .deps
                .iter()
                .map(|spec| content::encode_dep(spec))
                .collect::<Vec<_>>();
            specs.sort();
            let mut dep_line = format!(
                "deps {id} {} {} {}",
                package.name, package.version, classified.source
            );
            if !specs.is_empty() {
                dep_line.push(' ');
                dep_line.push_str(&specs.join(" "));
            }
            dep_lines.push(dep_line);
        }
    }

    let mut feature_lines = Vec::new();
    let mut activate_lines = Vec::new();
    let mut patch_lines = Vec::new();
    for member in &members {
        let manifest = if member == "." {
            root.join("Cargo.toml")
        } else {
            root.join(member).join("Cargo.toml")
        };
        println!("cargo:rerun-if-changed={}", manifest.display());
        let text = fs::read_to_string(&manifest).expect("member manifest");
        let (package, features, activates) = parse::parse_member_manifest(&text);
        for feature in features {
            assert!(!feature.contains('\n'), "feature body stays on one line");
            feature_lines.push(format!("feature {package} {feature}"));
        }
        activate_lines.extend(activates);
        patch_lines.extend(parse::parse_patches(&text));
    }
    for lock in &locks {
        if let Some(manifest) = lock.parent().map(|dir| dir.join("Cargo.toml"))
            && manifest.is_file()
            && manifest != root.join("Cargo.toml")
        {
            println!("cargo:rerun-if-changed={}", manifest.display());
            if let Ok(text) = fs::read_to_string(&manifest) {
                patch_lines.extend(parse::parse_patches(&text));
            }
        }
        println!("cargo:rerun-if-changed={}", lock.display());
    }
    patch_lines.extend(parse::parse_patches(&workspace_toml));
    println!(
        "cargo:rerun-if-changed={}",
        root.join("Cargo.toml").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join("Cargo.lock").display()
    );

    let mut generated_lines = Vec::new();
    let workspace_domain = domain_id(&root_name, ".");
    for path in &generated {
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = fs::read(path).expect("generated source");
        let rel = relative(&root, path);
        let domain = domains
            .iter()
            .filter(|domain| domain.canonical != ".")
            .find(|domain| rel.starts_with(&format!("{}/", domain.canonical)))
            .map(|domain| domain.id.clone())
            .unwrap_or_else(|| workspace_domain.clone());
        generated_lines.push(format!(
            "generated {domain} {rel} {} {}",
            bytes.len(),
            sha256::hex(&sha256::sha256(&bytes))
        ));
    }

    let (release, commit, host) = toolchain();
    let target = env_var("TARGET");
    let root_lock = fs::read_to_string(root.join("Cargo.lock")).expect("root lock");
    let (root_lock_version, _) = parse::parse_lock(&root_lock);

    let mut lines = vec!["sim-world-inputs-v1".to_owned()];
    lines.push(format!("config lock {root_lock_version}"));
    lines.push(format!("config resolver {resolver}"));
    lines.push(format!("root-package {ROOT_PACKAGE}"));
    lines.push(format!("sealed-target {target}"));
    lines.push(format!("toolchain commit {commit}"));
    lines.push(format!("toolchain host {host}"));
    lines.push(format!("toolchain release {release}"));
    feature_lines.sort();
    feature_lines.dedup();
    activate_lines.sort();
    activate_lines.dedup();
    patch_lines.sort();
    patch_lines.dedup();
    domains.sort_by(|left, right| left.id.cmp(&right.id));
    member_lines.sort();
    generated_lines.sort();
    lines.extend(feature_lines);
    lines.extend(activate_lines);
    lines.extend(patch_lines);
    for domain in &domains {
        lines.push(format!(
            "domain {} {} {} {}",
            domain.id, domain.canonical, domain.role, domain.lock
        ));
        let mut aliases = domain.aliases.clone();
        aliases.sort();
        aliases.dedup();
        for alias in aliases {
            lines.push(format!("alias {} {alias}", domain.id));
        }
    }
    dep_lines.sort();
    path_file_lines.sort();
    lines.extend(member_lines);
    lines.extend(dep_lines);
    lines.extend(path_file_lines);
    lines.extend(generated_lines);
    lines.push(String::new());

    let out_dir = PathBuf::from(env_var("OUT_DIR"));
    let destination = out_dir.join("owning-inputs.txt");
    fs::write(&destination, lines.join("\n")).expect("write owning inputs");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=build/parse.rs");
    println!("cargo:rerun-if-changed=build/sha256.rs");
}

struct Domain {
    id: String,
    canonical: String,
    role: String,
    lock: String,
    aliases: Vec<String>,
}

struct Pkg {
    name: String,
    version: String,
    source: String,
    checksum: String,
    deps: Vec<String>,
}

fn domain_id(root_name: &str, canonical: &str) -> String {
    if canonical == "." {
        format!("resolver/{root_name}")
    } else {
        format!("resolver/{root_name}/{canonical}")
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn collect_named(root: &Path, file_name: &str, out: &mut Vec<PathBuf>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries =
            fs::read_dir(&dir).unwrap_or_else(|error| panic!("read {}: {error}", dir.display()));
        for entry in entries {
            let entry = entry.unwrap_or_else(|error| panic!("dir entry: {error}"));
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if name == "target" || name == ".git" || name == ".sim" {
                    continue;
                }
                stack.push(path);
            } else if name == file_name {
                out.push(path);
            }
        }
    }
}

fn collect_generated(root: &Path, out: &mut Vec<PathBuf>) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries =
            fs::read_dir(&dir).unwrap_or_else(|error| panic!("read {}: {error}", dir.display()));
        for entry in entries {
            let entry = entry.unwrap_or_else(|error| panic!("dir entry: {error}"));
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if path.is_dir() {
                if name == "target" || name == ".git" || name == ".sim" {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if !name.ends_with(".rs") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            if text.lines().take(4).any(|line| line.contains("@generated")) {
                out.push(path);
            }
        }
    }
}

fn closure_indexes(packages: &[Pkg], root_package: &str) -> Vec<usize> {
    if root_package.is_empty() {
        return packages
            .iter()
            .enumerate()
            .filter(|(_, package)| package.source.is_empty())
            .map(|(index, _)| index)
            .collect();
    }
    let mut roots = Vec::new();
    for (index, package) in packages.iter().enumerate() {
        if package.name == root_package && package.source.is_empty() {
            roots.push(index);
        }
    }
    assert!(
        !roots.is_empty(),
        "root package {root_package} is in the lock"
    );
    let mut seen = vec![false; packages.len()];
    let mut queue = roots;
    while let Some(index) = queue.pop() {
        if seen[index] {
            continue;
        }
        seen[index] = true;
        for dep in &packages[index].deps {
            let Some(resolved) = resolve(packages, dep) else {
                panic!(
                    "unresolved lock dependency {dep} from {}",
                    packages[index].name
                );
            };
            queue.push(resolved);
        }
    }
    seen.iter()
        .enumerate()
        .filter(|(_, marked)| **marked)
        .map(|(index, _)| index)
        .collect()
}

fn resolve(packages: &[Pkg], spec: &str) -> Option<usize> {
    let (body, source) = if let Some(open) = spec.rfind(" (") {
        if let Some(stripped) = spec.strip_suffix(')') {
            (&stripped[..open], Some(&spec[open + 2..spec.len() - 1]))
        } else {
            (spec, None)
        }
    } else {
        (spec, None)
    };
    let mut parts = body.split_whitespace();
    let name = parts.next()?;
    let version = parts.next();
    let mut found = Vec::new();
    for (index, package) in packages.iter().enumerate() {
        if package.name != name {
            continue;
        }
        if version.is_some_and(|version| package.version != version) {
            continue;
        }
        if source.is_some_and(|source| package.source != source) {
            continue;
        }
        found.push(index);
    }
    if found.len() == 1 {
        return found.pop();
    }
    if source.is_none() {
        let paths: Vec<usize> = found
            .iter()
            .copied()
            .filter(|index| packages[*index].source.is_empty())
            .collect();
        if paths.len() == 1 {
            return paths.first().copied();
        }
    }
    if found.is_empty() {
        None
    } else {
        panic!("ambiguous lock dependency {spec}");
    }
}

fn origin_of(source: &str) -> &'static str {
    if source.starts_with("registry+") {
        "registry"
    } else if source.starts_with("git+") {
        "git"
    } else {
        "path"
    }
}

fn toolchain() -> (String, String, String) {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let output = Command::new(&rustc)
        .arg("-vV")
        .output()
        .unwrap_or_else(|error| panic!("run {rustc} -vV: {error}"));
    assert!(output.status.success(), "rustc -vV failed");
    let text = String::from_utf8(output.stdout).expect("rustc utf8");
    let mut release = String::new();
    let mut commit = String::new();
    let mut host = String::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("release: ") {
            release = value.to_owned();
        } else if let Some(value) = line.strip_prefix("commit-hash: ") {
            commit = value.to_owned();
        } else if let Some(value) = line.strip_prefix("host: ") {
            host = value.to_owned();
        }
    }
    assert!(!release.is_empty() && !commit.is_empty() && !host.is_empty());
    (release, commit, host)
}

fn env_var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is set by cargo"))
}
