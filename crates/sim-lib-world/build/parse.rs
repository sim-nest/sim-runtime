// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Workspace, member, and lock parsers for the owning input text.

use super::Pkg;

pub(super) fn parse_workspace(text: &str) -> (String, Vec<String>) {
    let mut section = String::new();
    let mut resolver = String::new();
    for line in text.lines() {
        let trim = line.trim();
        if let Some(header) = section_header(trim) {
            section = header;
            continue;
        }
        if section == "workspace"
            && let Some(value) = quoted_assign(trim, "resolver")
        {
            resolver = value;
        }
    }
    assert!(!resolver.is_empty(), "workspace resolver");
    (resolver, workspace_members(text))
}

pub(super) fn workspace_members(text: &str) -> Vec<String> {
    let mut section = String::new();
    let mut members = Vec::new();
    let mut in_members = false;
    for line in text.lines() {
        let trim = line.trim();
        if let Some(header) = section_header(trim) {
            section = header;
            in_members = false;
            continue;
        }
        if section != "workspace" {
            continue;
        }
        if trim.starts_with("members") {
            in_members = true;
        }
        if in_members {
            if trim.starts_with(']') {
                in_members = false;
            } else if let Some(member) = first_quoted(trim) {
                members.push(member);
            }
        }
    }
    members
}

pub(super) fn package_identity(text: &str) -> Option<(String, String)> {
    let mut section = String::new();
    let mut name = String::new();
    let mut version = String::new();
    for line in text.lines() {
        let trim = line.trim();
        if let Some(header) = section_header(trim) {
            section = header;
            continue;
        }
        if section != "package" {
            continue;
        }
        if let Some(value) = quoted_assign(trim, "name") {
            name = value;
        } else if let Some(value) = quoted_assign(trim, "version") {
            version = value;
        }
    }
    if name.is_empty() || version.is_empty() {
        None
    } else {
        Some((name, version))
    }
}

pub(super) fn path_declarations(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in text.lines() {
        if let Some(value) = path_assign(line.trim()) {
            found.push(value);
        }
    }
    found
}

pub(super) fn parse_member_manifest(text: &str) -> (String, Vec<String>, Vec<String>) {
    let mut section = String::new();
    let mut package = String::new();
    let mut features = Vec::new();
    let mut activates = Vec::new();
    let mut table_dep = String::new();
    let mut feature_name = String::new();
    let mut feature_body = String::new();
    let mut in_feature = false;
    for line in text.lines() {
        let trim = line.trim();
        if let Some(header) = section_header(trim) {
            if in_feature {
                features.push(format!("{feature_name} {}", squash(&feature_body)));
                in_feature = false;
            }
            section = header;
            table_dep.clear();
            if let Some(name) = section.strip_prefix("dependencies.") {
                table_dep = name.to_owned();
            } else if let Some(name) = section.strip_prefix("dev-dependencies.") {
                table_dep = name.to_owned();
            } else if let Some(name) = section.strip_prefix("build-dependencies.") {
                table_dep = name.to_owned();
            }
            continue;
        }
        if section == "package" {
            if let Some(name) = quoted_assign(trim, "name") {
                package = name;
            }
        } else if section == "features" {
            if in_feature {
                feature_body.push_str(trim);
                if trim.contains(']') {
                    features.push(format!("{feature_name} {}", squash(&feature_body)));
                    in_feature = false;
                }
                continue;
            }
            if let Some((name, rest)) = trim.split_once('=') {
                let name = name.trim();
                if name.is_empty() || name.starts_with('#') {
                    continue;
                }
                if rest.contains('[') && !rest.contains(']') {
                    feature_name = name.to_owned();
                    feature_body = rest.to_owned();
                    in_feature = true;
                } else {
                    features.push(format!("{name} {}", squash(rest)));
                }
            }
        } else if let Some(scope) = dependency_scope(&section) {
            let dep = if table_dep.is_empty() {
                trim.split_once('=')
                    .map(|(name, _)| name.trim().trim_matches('"').to_owned())
                    .unwrap_or_default()
            } else {
                table_dep.clone()
            };
            if dep.is_empty() {
                continue;
            }
            for feature in quoted_list_after(trim, "features") {
                activates.push(format!("activate {package} {dep} {feature} {scope}"));
            }
        }
    }
    if in_feature {
        features.push(format!("{feature_name} {}", squash(&feature_body)));
    }
    assert!(!package.is_empty(), "member package name");
    (package, features, activates)
}

fn squash(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join("")
}

pub(super) fn parse_patches(text: &str) -> Vec<String> {
    let mut section = String::new();
    let mut lines = Vec::new();
    for line in text.lines() {
        let trim = line.trim();
        if let Some(header) = section_header(trim) {
            section = header;
            continue;
        }
        let Some(registry) = section.strip_prefix("patch.") else {
            continue;
        };
        let Some((name, source)) = trim.split_once('=') else {
            continue;
        };
        let name = name.trim().trim_matches('"');
        if name.is_empty() || name.starts_with('#') {
            continue;
        }
        let source = source.split_whitespace().collect::<Vec<_>>().join("~");
        lines.push(format!("patch {registry} {name} {source}"));
    }
    lines
}

fn dependency_scope(section: &str) -> Option<&'static str> {
    if section == "dependencies" || section.starts_with("dependencies.") {
        Some("normal")
    } else if section == "dev-dependencies" || section.starts_with("dev-dependencies.") {
        Some("dev")
    } else if section == "build-dependencies" || section.starts_with("build-dependencies.") {
        Some("build")
    } else {
        None
    }
}

pub(super) fn parse_lock(text: &str) -> (String, Vec<Pkg>) {
    let mut lock_version = String::new();
    let mut packages = Vec::new();
    let mut current: Option<Pkg> = None;
    let mut in_deps = false;
    for line in text.lines() {
        let trim = line.trim();
        if trim == "[[package]]" {
            if let Some(package) = current.take() {
                packages.push(package);
            }
            current = Some(Pkg {
                name: String::new(),
                version: String::new(),
                source: String::new(),
                checksum: String::new(),
                deps: Vec::new(),
            });
            in_deps = false;
            continue;
        }
        if current.is_none() {
            if lock_version.is_empty()
                && let Some(value) = trim.strip_prefix("version = ")
            {
                lock_version = value.trim().trim_matches('"').to_owned();
            }
            continue;
        }
        if in_deps {
            if trim.starts_with(']') {
                in_deps = false;
            } else if let Some(dep) = first_quoted(trim) {
                current.as_mut().expect("package").deps.push(dep);
            }
            continue;
        }
        if trim.starts_with("dependencies") {
            in_deps = trim.contains('[') && !trim.contains(']');
            continue;
        }
        let package = current.as_mut().expect("package");
        if let Some(value) = quoted_assign(trim, "name") {
            package.name = value;
        } else if let Some(value) = quoted_assign(trim, "version") {
            package.version = value;
        } else if let Some(value) = quoted_assign(trim, "source") {
            package.source = value;
        } else if let Some(value) = quoted_assign(trim, "checksum") {
            package.checksum = value;
        }
    }
    if let Some(package) = current {
        packages.push(package);
    }
    if lock_version.is_empty() {
        lock_version = "unknown".to_owned();
    }
    (lock_version, packages)
}

fn section_header(line: &str) -> Option<String> {
    let line = line.trim();
    if line.starts_with('[') && line.ends_with(']') && !line.starts_with("[[") {
        Some(line[1..line.len() - 1].to_owned())
    } else {
        None
    }
}

fn path_assign(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut start = 0;
    while let Some(offset) = line[start..].find("path") {
        let at = start + offset;
        let boundary = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
        start = at + 4;
        if !boundary {
            continue;
        }
        let rest = line[start..].trim_start();
        if let Some(rest) = rest.strip_prefix('=')
            && let Some(value) = first_quoted(rest.trim_start())
        {
            return Some(value);
        }
    }
    None
}

fn quoted_assign(line: &str, key: &str) -> Option<String> {
    let rest = line.strip_prefix(key)?.trim_start();
    let rest = rest.strip_prefix('=')?.trim();
    first_quoted(rest).or_else(|| {
        let token = rest.split_whitespace().next()?;
        Some(token.trim_matches('"').to_owned())
    })
}

fn first_quoted(line: &str) -> Option<String> {
    let start = line.find('"')?;
    let tail = &line[start + 1..];
    let end = tail.find('"')?;
    Some(tail[..end].to_owned())
}

fn quoted_list_after(line: &str, key: &str) -> Vec<String> {
    let Some(position) = line.find(key) else {
        return Vec::new();
    };
    let Some(start) = line[position..].find('[') else {
        return Vec::new();
    };
    let Some(end) = line[position + start..].find(']') else {
        return Vec::new();
    };
    let body = &line[position + start + 1..position + start + end];
    let mut values = Vec::new();
    let mut rest = body;
    while let Some(begin) = rest.find('"') {
        let tail = &rest[begin + 1..];
        let Some(stop) = tail.find('"') else {
            break;
        };
        values.push(tail[..stop].to_owned());
        rest = &tail[stop + 1..];
    }
    values
}
