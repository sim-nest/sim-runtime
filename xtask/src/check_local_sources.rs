//! Verifies that workspace crates depend on sibling workspace crates by local
//! path-plus-version, not by crates.io name alone.

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    name: String,
    manifest_path: String,
    dependencies: Vec<Dependency>,
}

#[derive(Debug, Deserialize)]
struct Dependency {
    name: String,
    path: Option<String>,
}

pub fn run(args: Vec<String>) -> Result<(), String> {
    let program = args.first().map(String::as_str).unwrap_or("xtask");
    if args.get(1).map(String::as_str) != Some("check-local-sources") || args.len() != 2 {
        return Err(format!("usage: {program} check-local-sources"));
    }

    let repo_root = repo_root()?;
    let metadata = load_metadata(&repo_root)?;
    let errors = duplicate_source_errors(&metadata, &repo_root);
    if errors.is_empty() {
        println!("check-local-sources: OK");
        return Ok(());
    }

    Err(format!(
        "workspace crates must depend on sibling workspace crates via path-plus-version:\n{}",
        errors.join("\n")
    ))
}

fn repo_root() -> Result<PathBuf, String> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask manifest should have a workspace root parent".to_owned())
}

fn load_metadata(repo_root: &Path) -> Result<Metadata, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .current_dir(repo_root)
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|err| format!("run cargo metadata: {err}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(|err| format!("parse cargo metadata: {err}"))
}

fn duplicate_source_errors(metadata: &Metadata, repo_root: &Path) -> Vec<String> {
    let workspace_members: BTreeSet<&str> = metadata
        .workspace_members
        .iter()
        .map(String::as_str)
        .collect();
    let workspace_packages: Vec<&Package> = metadata
        .packages
        .iter()
        .filter(|package| workspace_members.contains(package.id.as_str()))
        .collect();
    let workspace_names: BTreeSet<&str> = workspace_packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();

    let mut errors = Vec::new();
    for package in workspace_packages {
        for dependency in &package.dependencies {
            if dependency.path.is_some() || dependency.name == package.name {
                continue;
            }
            if workspace_names.contains(dependency.name.as_str()) {
                errors.push(format!(
                    "- {} depends on workspace crate `{}` without a local `path` entry",
                    relpath(repo_root, &package.manifest_path),
                    dependency.name
                ));
            }
        }
    }
    errors
}

fn relpath(repo_root: &Path, manifest_path: &str) -> String {
    Path::new(manifest_path)
        .strip_prefix(repo_root)
        .unwrap_or_else(|_| Path::new(manifest_path))
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(
        id: &str,
        name: &str,
        manifest_path: &str,
        dependencies: Vec<Dependency>,
    ) -> Package {
        Package {
            id: id.to_owned(),
            name: name.to_owned(),
            manifest_path: manifest_path.to_owned(),
            dependencies,
        }
    }

    fn dependency(name: &str, path: Option<&str>) -> Dependency {
        Dependency {
            name: name.to_owned(),
            path: path.map(str::to_owned),
        }
    }

    #[test]
    fn reports_workspace_dependency_without_path() {
        let metadata = Metadata {
            packages: vec![
                package(
                    "pkg-a 0.1.0 (path+file:///repo/crates/pkg-a)",
                    "pkg-a",
                    "/repo/crates/pkg-a/Cargo.toml",
                    vec![dependency("pkg-b", None)],
                ),
                package(
                    "pkg-b 0.1.0 (path+file:///repo/crates/pkg-b)",
                    "pkg-b",
                    "/repo/crates/pkg-b/Cargo.toml",
                    Vec::new(),
                ),
            ],
            workspace_members: vec![
                "pkg-a 0.1.0 (path+file:///repo/crates/pkg-a)".to_owned(),
                "pkg-b 0.1.0 (path+file:///repo/crates/pkg-b)".to_owned(),
            ],
        };

        let errors = duplicate_source_errors(&metadata, Path::new("/repo"));

        assert_eq!(
            errors,
            vec![
                "- crates/pkg-a/Cargo.toml depends on workspace crate `pkg-b` without a local `path` entry"
            ]
        );
    }

    #[test]
    fn ignores_workspace_dependency_with_path() {
        let metadata = Metadata {
            packages: vec![
                package(
                    "pkg-a 0.1.0 (path+file:///repo/crates/pkg-a)",
                    "pkg-a",
                    "/repo/crates/pkg-a/Cargo.toml",
                    vec![dependency("pkg-b", Some("../pkg-b"))],
                ),
                package(
                    "pkg-b 0.1.0 (path+file:///repo/crates/pkg-b)",
                    "pkg-b",
                    "/repo/crates/pkg-b/Cargo.toml",
                    Vec::new(),
                ),
            ],
            workspace_members: vec![
                "pkg-a 0.1.0 (path+file:///repo/crates/pkg-a)".to_owned(),
                "pkg-b 0.1.0 (path+file:///repo/crates/pkg-b)".to_owned(),
            ],
        };

        assert!(duplicate_source_errors(&metadata, Path::new("/repo")).is_empty());
    }
}
