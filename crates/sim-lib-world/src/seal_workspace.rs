// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Workspace selection for the locked compile probe.
//!
//! The manifest directory is the workspace only for a source build. The
//! registry checkout that publishes this crate has no workspace manifest.

use std::path::{Path, PathBuf};

pub(crate) fn resolve(baked: PathBuf, cwd: Option<&Path>) -> PathBuf {
    if baked.join("Cargo.toml").is_file() {
        return baked;
    }
    if let Some(cwd) = cwd {
        let mut current = Some(cwd);
        while let Some(dir) = current {
            if binds_world(dir) {
                return dir.to_path_buf();
            }
            current = dir.parent();
        }
    }
    baked
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
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if !trimmed.starts_with("members") || trimmed.starts_with("default-members") {
            continue;
        }
        let mut body = String::new();
        if let Some(open) = trimmed.find('[') {
            body.push_str(&trimmed[open..]);
        }
        if !body.contains(']') {
            for next in lines.by_ref() {
                body.push('\n');
                body.push_str(next);
                if next.contains(']') {
                    break;
                }
            }
        }
        let Some(open) = body.find('[') else {
            continue;
        };
        let Some(close) = body[open..].find(']') else {
            continue;
        };
        for quoted in quotes(&body[open + 1..open + close]) {
            if quoted.is_empty()
                || quoted.contains("..")
                || quoted.starts_with('/')
                || quoted.contains('\\')
            {
                continue;
            }
            dirs.push(workspace.join(quoted));
        }
    }
    dirs
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
