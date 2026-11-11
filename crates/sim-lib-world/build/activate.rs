// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Whether one lock edge is an input of this target.
//!
//! Cargo.lock lists dev-dependencies and target-specific dependencies of
//! every package. This build only downloaded the edges that compile here.
//! An edge that does not compile on this target stays outside the closure.

use std::collections::BTreeSet;

pub(super) fn dep_active(manifest: &str, spec: &str, triple: &str, follow_dev: bool) -> bool {
    let Some(name) = spec.split_whitespace().next() else {
        return false;
    };
    active_names(manifest, triple, follow_dev).contains(name)
}

fn active_names(manifest: &str, triple: &str, follow_dev: bool) -> BTreeSet<String> {
    let target = Triple::parse(triple);
    let mut names = BTreeSet::new();
    let mut section = Section::other();
    let mut depth = 0_i32;
    let mut pending = Vec::new();
    for line in manifest.lines() {
        let trim = line.trim();
        if depth == 0
            && let Some(header) = section_header(trim)
        {
            section = Section::parse(&header);
            if let Some(name) = section.table.clone() {
                remember(&mut names, &section, &target, follow_dev, &name);
            }
            continue;
        }
        if section.class == Class::Other {
            continue;
        }
        if depth == 0
            && let Some(key) = assign_key(trim)
            && !is_field(key)
        {
            pending.push(key.to_owned());
        }
        if let Some(package) = package_rename(trim) {
            pending.push(package);
        }
        depth += brace_delta(trim);
        if depth > 0 {
            continue;
        }
        depth = 0;
        for name in pending.drain(..) {
            remember(&mut names, &section, &target, follow_dev, &name);
        }
    }
    names
}

fn remember(
    names: &mut BTreeSet<String>,
    section: &Section,
    target: &Triple,
    follow_dev: bool,
    name: &str,
) {
    if section.active(target, follow_dev) && !name.is_empty() && !is_field(name) {
        names.insert(name.to_owned());
    }
}

fn is_field(key: &str) -> bool {
    matches!(
        key,
        "version"
            | "features"
            | "optional"
            | "default-features"
            | "path"
            | "package"
            | "registry"
            | "git"
            | "branch"
            | "rev"
            | "tag"
            | "workspace"
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Normal,
    Build,
    Dev,
    Other,
}

struct Section {
    class: Class,
    cfg: Option<String>,
    table: Option<String>,
}

impl Section {
    fn other() -> Self {
        Self {
            class: Class::Other,
            cfg: None,
            table: None,
        }
    }

    fn parse(header: &str) -> Self {
        let (cfg, body) = split_target(header)
            .map(|(cfg, body)| (Some(cfg), body))
            .unwrap_or_else(|| (None, header.to_owned()));
        let (class, table) = dependency_section(&body);
        Self { class, cfg, table }
    }

    fn active(&self, target: &Triple, follow_dev: bool) -> bool {
        let class_ok = match self.class {
            Class::Normal | Class::Build => true,
            Class::Dev => follow_dev,
            Class::Other => false,
        };
        class_ok && self.cfg.as_deref().is_none_or(|cfg| cfg_holds(cfg, target))
    }
}

fn dependency_section(body: &str) -> (Class, Option<String>) {
    for (prefix, class) in [
        ("dev-dependencies", Class::Dev),
        ("build-dependencies", Class::Build),
        ("dependencies", Class::Normal),
    ] {
        if let Some(name) = body.strip_prefix(prefix) {
            let name = name.strip_prefix('.').unwrap_or(name);
            let table = if name.is_empty() {
                None
            } else {
                Some(name.to_owned())
            };
            return (class, table);
        }
    }
    (Class::Other, None)
}

fn split_target(header: &str) -> Option<(String, String)> {
    let rest = header.strip_prefix("target.")?;
    let quote = rest.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let rest = &rest[quote.len_utf8()..];
    let end = rest.find(quote)?;
    let cfg = rest[..end].to_owned();
    let after = rest[end + quote.len_utf8()..].trim_start_matches('.');
    Some((cfg, after.to_owned()))
}

fn section_header(line: &str) -> Option<String> {
    if line.starts_with('[') && line.ends_with(']') && !line.starts_with("[[") {
        Some(line[1..line.len() - 1].to_owned())
    } else {
        None
    }
}

fn assign_key(line: &str) -> Option<&str> {
    if line.starts_with('#') {
        return None;
    }
    let key = line.split('=').next()?.trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return None;
    }
    Some(key)
}

fn package_rename(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut start = 0;
    while let Some(offset) = line[start..].find("package") {
        let at = start + offset;
        let boundary = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        start = at + "package".len();
        if !boundary {
            continue;
        }
        let rest = line[start..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(rest) = rest.strip_prefix('"') else {
            continue;
        };
        let end = rest.find('"')?;
        return Some(rest[..end].to_owned());
    }
    None
}

fn brace_delta(line: &str) -> i32 {
    let open = i32::try_from(line.matches('{').count()).unwrap_or(0);
    let close = i32::try_from(line.matches('}').count()).unwrap_or(0);
    open - close
}

struct Triple {
    arch: String,
    os: String,
    env: String,
    vendor: String,
}

impl Triple {
    fn parse(text: &str) -> Self {
        let mut parts = text.split('-');
        Self {
            arch: parts.next().unwrap_or("").to_owned(),
            vendor: parts.next().unwrap_or("").to_owned(),
            os: parts.next().unwrap_or("").to_owned(),
            env: parts.next().unwrap_or("").to_owned(),
        }
    }

    fn unix(&self) -> bool {
        matches!(
            self.os.as_str(),
            "linux"
                | "android"
                | "freebsd"
                | "netbsd"
                | "openbsd"
                | "dragonfly"
                | "macos"
                | "ios"
                | "darwin"
                | "illumos"
                | "solaris"
                | "hurd"
                | "redox"
                | "aix"
                | "haiku"
                | "nuttx"
                | "nto"
                | "cygwin"
        )
    }

    fn windows(&self) -> bool {
        self.os == "windows"
    }

    fn family(&self, name: &str) -> bool {
        match name {
            "unix" => self.unix(),
            "windows" => self.windows(),
            "wasm" => self.arch.starts_with("wasm"),
            _ => true,
        }
    }
}

fn cfg_holds(expr: &str, target: &Triple) -> bool {
    let expr = expr.trim();
    if let Some(inner) = strip_call(expr, "cfg") {
        pred_holds(inner, target)
    } else {
        pred_holds(expr, target)
    }
}

fn pred_holds(expr: &str, target: &Triple) -> bool {
    let expr = expr.trim();
    if let Some(inner) = strip_call(expr, "all") {
        return split_args(inner)
            .iter()
            .all(|part| pred_holds(part, target));
    }
    if let Some(inner) = strip_call(expr, "any") {
        return split_args(inner)
            .iter()
            .any(|part| pred_holds(part, target));
    }
    if let Some(inner) = strip_call(expr, "not") {
        return !pred_holds(inner, target);
    }
    if expr == "unix" {
        return target.unix();
    }
    if expr == "windows" {
        return target.windows();
    }
    let Some((key, raw)) = expr.split_once('=') else {
        return true;
    };
    let value = raw.trim().trim_matches('"');
    match key.trim() {
        "target_os" => target.os == value,
        "target_arch" => target.arch == value,
        "target_vendor" => target.vendor == value,
        "target_env" => target.env == value,
        "target_family" => target.family(value),
        _ => true,
    }
}

fn strip_call<'a>(expr: &'a str, name: &str) -> Option<&'a str> {
    let rest = expr.strip_prefix(name)?.trim_start();
    let rest = rest.strip_prefix('(')?;
    rest.strip_suffix(')').map(str::trim)
}

fn split_args(inner: &str) -> Vec<&str> {
    let mut args = Vec::new();
    let mut depth = 0_i32;
    let mut start = 0;
    for (index, ch) in inner.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                args.push(inner[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    let tail = inner[start..].trim();
    if !tail.is_empty() {
        args.push(tail);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::dep_active;

    const LINUX: &str = "x86_64-unknown-linux-gnu";

    const INVENTORY: &str = r#"
[dev-dependencies.rustversion]
version = "1.0"

[target.'cfg(target_family = "wasm")'.dependencies.rustversion]
version = "1.0"
"#;

    const LIBLOADING: &str = r#"
[target."cfg(unix)".dependencies.cfg-if]
version = "1"

[target."cfg(windows)".dependencies.windows-link]
version = "0.2"
"#;

    const RENAMED: &str = r#"
[dependencies]
sha2 = "0.10"
sim-conformance-evidence = { package = "sim-conformance-core", version = "0.2.0" }
renamed = {
    package = "actual",
    version = "1",
}

[dev-dependencies]
rustversion = "1.0"
"#;

    #[test]
    fn dev_and_foreign_target_edges_stay_inactive() {
        assert!(!dep_active(INVENTORY, "rustversion", LINUX, false));
        assert!(dep_active(
            INVENTORY,
            "rustversion",
            "wasm32-unknown-unknown",
            false
        ));
        assert!(dep_active(LIBLOADING, "cfg-if", LINUX, false));
        assert!(!dep_active(LIBLOADING, "windows-link", LINUX, false));
        assert!(dep_active(
            LIBLOADING,
            "windows-link",
            "x86_64-pc-windows-msvc",
            false
        ));
    }

    #[test]
    fn renamed_packages_and_dev_edges_follow_the_declared_class() {
        assert!(dep_active(RENAMED, "sha2", LINUX, false));
        assert!(dep_active(
            RENAMED,
            "sim-conformance-core 0.2.0",
            LINUX,
            false
        ));
        assert!(dep_active(RENAMED, "actual", LINUX, false));
        assert!(!dep_active(RENAMED, "rustversion", LINUX, false));
        assert!(dep_active(RENAMED, "rustversion", LINUX, true));
    }
}
