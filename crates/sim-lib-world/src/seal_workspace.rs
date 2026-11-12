// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Workspace selection for the locked compile probe.
//!
//! A source build keeps the two-level workspace path when that directory
//! has a manifest. A published crate walks from the process directory.

use std::path::{Path, PathBuf};

pub(crate) fn resolve(baked: PathBuf, cwd: Option<&Path>) -> PathBuf {
    if baked.join("Cargo.toml").is_file() {
        return baked;
    }
    let mut selected = None;
    if let Some(cwd) = cwd {
        let mut current = Some(cwd);
        while let Some(dir) = current {
            if binds_world(dir) {
                selected = Some(dir.to_path_buf());
            }
            current = dir.parent();
        }
    }
    selected.unwrap_or(baked)
}

fn binds_world(dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join("Cargo.toml")) else {
        return false;
    };
    if text.contains("sim-lib-world") {
        return true;
    }
    member_dirs(dir, &text).into_iter().any(|member| {
        std::fs::read_to_string(member.join("Cargo.toml"))
            .unwrap_or_default()
            .contains("sim-lib-world")
    })
}

fn member_dirs(workspace: &Path, text: &str) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut section = String::new();
    let mut in_members = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(header) = table_header(trimmed) {
            section = header.to_owned();
            in_members = false;
            continue;
        }
        if section != "workspace" {
            continue;
        }
        if key_is(trimmed, "members") {
            in_members = true;
        }
        if !in_members {
            continue;
        }
        if trimmed.starts_with(']') {
            in_members = false;
            continue;
        }
        for quoted in quotes(trimmed) {
            if quoted.is_empty()
                || quoted.contains("..")
                || quoted.starts_with('/')
                || quoted.contains('\\')
            {
                continue;
            }
            dirs.push(workspace.join(quoted));
        }
        if trimmed.contains(']') {
            in_members = false;
        }
    }
    dirs
}

fn table_header(line: &str) -> Option<&str> {
    if line.starts_with('[') && line.ends_with(']') && !line.starts_with("[[") {
        Some(&line[1..line.len() - 1])
    } else {
        None
    }
}

fn key_is(line: &str, key: &str) -> bool {
    line.strip_prefix(key)
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

fn quotes(body: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = body;
    while let Some(start) = rest.find('"') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('"') else {
            break;
        };
        found.push(&after[..end]);
        rest = &after[end + 1..];
    }
    found
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::resolve;
    use crate::seal_compile::run_locked_check;

    static TEMP: AtomicU64 = AtomicU64::new(0);

    struct Scratch {
        root: PathBuf,
    }

    impl Scratch {
        fn new() -> Self {
            let n = TEMP.fetch_add(1, Ordering::Relaxed);
            let root =
                std::env::temp_dir().join(format!("sim-seal-workspace-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("scratch");
            Self { root }
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.root.join(rel)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn write(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent");
        }
        fs::write(path, text).expect("write");
    }

    #[test]
    fn source_manifest_stays_the_baked_workspace() {
        let scratch = Scratch::new();
        let baked = scratch.path("source");
        write(&baked.join("Cargo.toml"), "[workspace]\nmembers = []\n");
        let elsewhere = scratch.path("elsewhere");
        write(
            &elsewhere.join("Cargo.toml"),
            "sim-lib-world = { version = \"0.1.8\" }\n",
        );
        assert_eq!(resolve(baked.clone(), Some(&elsewhere)), baked);
    }

    #[test]
    fn published_layout_uses_the_package_that_names_the_crate() {
        let scratch = Scratch::new();
        let baked = scratch.path("registry/src");
        fs::create_dir_all(&baked).expect("registry");
        let package = scratch.path("checkout/crates/sim-run");
        write(
            &package.join("Cargo.toml"),
            "[package]\nname = \"sim-run\"\nsim-lib-world = { version = \"0.1.8\" }\n",
        );
        assert_eq!(resolve(baked, Some(&package)), package);
    }

    #[test]
    fn published_layout_uses_a_workspace_member_that_names_the_crate() {
        let scratch = Scratch::new();
        let baked = scratch.path("registry/src");
        fs::create_dir_all(&baked).expect("registry");
        let workspace = scratch.path("checkout");
        write(
            &workspace.join("Cargo.toml"),
            "[workspace]\nmembers = [\n  \"crates/sim-run\",\n  \"../outside\",\n]\n",
        );
        let package = workspace.join("crates/sim-run");
        write(
            &package.join("Cargo.toml"),
            "[package]\nname = \"sim-run\"\nsim-lib-world = { version = \"0.1.8\" }\n",
        );
        assert_eq!(resolve(baked, Some(&workspace)), workspace);
    }

    #[test]
    fn member_directory_uses_the_workspace_above_it() {
        let scratch = Scratch::new();
        let baked = scratch.path("registry/src");
        fs::create_dir_all(&baked).expect("registry");
        let workspace = scratch.path("checkout");
        write(
            &workspace.join("Cargo.toml"),
            "[workspace]\nmembers = [\n  \"crates/sim-run\",\n]\n",
        );
        let package = workspace.join("crates/sim-run");
        write(
            &package.join("Cargo.toml"),
            "[package]\nname = \"sim-run\"\nsim-lib-world = { version = \"0.1.8\" }\n",
        );
        assert_eq!(resolve(baked, Some(&package)), workspace);
    }

    #[test]
    fn members_outside_the_workspace_table_stay_unbound() {
        let scratch = Scratch::new();
        let baked = scratch.path("registry/src");
        fs::create_dir_all(&baked).expect("registry");
        let decoy = scratch.path("checkout/decoy");
        write(
            &decoy.join("Cargo.toml"),
            "[package]\nname = \"decoy\"\nmembers = [\"nested\"]\n[workspace]\nmembership = [\"nested\"]\n",
        );
        write(
            &decoy.join("nested/Cargo.toml"),
            "sim-lib-world = { version = \"0.1.8\" }\n",
        );
        assert_eq!(resolve(baked.clone(), Some(&decoy)), baked);
    }

    #[test]
    fn missing_name_keeps_the_baked_path() {
        let scratch = Scratch::new();
        let baked = scratch.path("registry/src");
        fs::create_dir_all(&baked).expect("registry");
        let cwd = scratch.path("empty/nested");
        write(&cwd.join("Cargo.toml"), "[package]\nname = \"other\"\n");
        assert_eq!(resolve(baked.clone(), Some(&cwd)), baked);
    }

    #[test]
    fn source_workspace_root_has_this_crate() {
        let root = crate::seal::workspace_root();
        assert!(root.join("Cargo.toml").is_file(), "{}", root.display());
        assert!(
            root.join("crates/sim-lib-world/Cargo.toml").is_file(),
            "{}",
            root.display()
        );
    }

    #[test]
    fn missing_manifest_does_not_start_cargo() {
        let scratch = Scratch::new();
        let empty = scratch.path("no-manifest");
        fs::create_dir_all(&empty).expect("empty");
        assert!(run_locked_check(&empty).is_err());
    }
}
