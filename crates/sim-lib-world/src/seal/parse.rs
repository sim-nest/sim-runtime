// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Parser for the sealed resolver-input text.

use std::collections::{BTreeMap, BTreeSet};

use super::{Domain, Generated, Inputs, Member};
use crate::WorldError;

pub(super) fn parse(text: &str) -> Result<Inputs, WorldError> {
    let mut inputs = Inputs {
        config: BTreeMap::new(),
        toolchain: BTreeMap::new(),
        features: BTreeSet::new(),
        activates: BTreeSet::new(),
        patches: BTreeSet::new(),
        sealed_target: String::new(),
        domains: BTreeMap::new(),
        members: Vec::new(),
        generated: Vec::new(),
    };
    let mut saw_header = false;
    let mut pending_deps = BTreeMap::<(String, String, String, String), Vec<String>>::new();
    let mut pending_files = BTreeMap::<(String, String), u64>::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if !saw_header {
            if line != "sim-world-inputs-v1" {
                return Err(WorldError::Qualification(
                    "source input header is missing".to_owned(),
                ));
            }
            saw_header = true;
            continue;
        }
        let parts: Vec<&str> = line.split(' ').filter(|part| !part.is_empty()).collect();
        match parts.as_slice() {
            ["config", key, value] => {
                inputs.config.insert((*key).to_owned(), (*value).to_owned());
            }
            ["toolchain", key, value] => {
                inputs
                    .toolchain
                    .insert((*key).to_owned(), (*value).to_owned());
            }
            ["feature", package, name, body @ ..] => {
                let body = body.join(" ");
                if body.is_empty() {
                    inputs.features.insert(format!("{package} {name}"));
                } else {
                    inputs.features.insert(format!("{package} {name} {body}"));
                }
            }
            ["activate", package, dep, feature, scope] => {
                inputs
                    .activates
                    .insert(format!("{package} {dep} {feature} {scope}"));
            }
            ["patch", registry, name, source] => {
                inputs.patches.insert(format!("{registry} {name} {source}"));
            }
            ["sealed-target", target] => inputs.sealed_target = (*target).to_owned(),
            ["root-package", _name] => {}
            ["domain", id, canonical, role, lock] => {
                inputs.domains.insert(
                    (*id).to_owned(),
                    Domain {
                        canonical: (*canonical).to_owned(),
                        role: (*role).to_owned(),
                        lock: (*lock).to_owned(),
                        aliases: BTreeSet::new(),
                    },
                );
            }
            ["alias", id, spelling] => {
                let domain = inputs.domains.get_mut(*id).ok_or_else(|| {
                    WorldError::Qualification(format!("alias for unknown domain {id}"))
                })?;
                domain.aliases.insert((*spelling).to_owned());
            }
            ["spell", raw, role, lock] => {
                let canonical = canonicalize_root(raw);
                inputs
                    .domains
                    .entry(canonical.clone())
                    .or_insert_with(|| Domain {
                        canonical: canonical.clone(),
                        role: (*role).to_owned(),
                        lock: (*lock).to_owned(),
                        aliases: BTreeSet::new(),
                    })
                    .aliases
                    .insert((*raw).to_owned());
            }
            [
                "member",
                domain,
                origin,
                name,
                version,
                source,
                checksum,
                kind,
                closure,
            ] => {
                let compiling = inputs
                    .domains
                    .get(*domain)
                    .ok_or_else(|| {
                        WorldError::Qualification(format!("member in unknown domain {domain}"))
                    })?
                    .role
                    == "compiling";
                inputs.members.push(member_row(MemberParts {
                    domain,
                    origin,
                    name,
                    version,
                    source,
                    checksum,
                    kind,
                    closure,
                    observed: "",
                    compiling,
                }));
            }
            [
                "member",
                domain,
                origin,
                name,
                version,
                source,
                checksum,
                kind,
                closure,
                observed,
            ] => {
                let compiling = inputs
                    .domains
                    .get(*domain)
                    .ok_or_else(|| {
                        WorldError::Qualification(format!("member in unknown domain {domain}"))
                    })?
                    .role
                    == "compiling";
                inputs.members.push(member_row(MemberParts {
                    domain,
                    origin,
                    name,
                    version,
                    source,
                    checksum,
                    kind,
                    closure,
                    observed,
                    compiling,
                }));
            }
            ["deps", domain, name, version, source, specs @ ..] => {
                let decoded = specs.iter().map(|spec| spec.replace('~', " ")).collect();
                let key = (
                    (*domain).to_owned(),
                    (*name).to_owned(),
                    (*version).to_owned(),
                    (*source).to_owned(),
                );
                if let Some(member) = inputs.members.iter_mut().find(|member| {
                    member.domain == key.0
                        && member.name == key.1
                        && member.version == key.2
                        && member.source == key.3
                }) {
                    member.deps = decoded;
                } else {
                    pending_deps.insert(key, decoded);
                }
            }
            ["path-files", domain, source, count] => {
                let count = count.parse().map_err(|_| {
                    WorldError::Qualification(format!("path file count is not a count: {count}"))
                })?;
                let key = ((*domain).to_owned(), (*source).to_owned());
                if let Some(member) = inputs
                    .members
                    .iter_mut()
                    .find(|member| member.domain == key.0 && member.source == key.1)
                {
                    member.files = Some(count);
                } else {
                    pending_files.insert(key, count);
                }
            }
            ["generated", domain, path, bytes, checksum] => {
                let bytes = bytes.parse().map_err(|_| {
                    WorldError::Qualification(format!("generated length is not a count: {bytes}"))
                })?;
                inputs.generated.push(Generated {
                    domain: (*domain).to_owned(),
                    path: (*path).to_owned(),
                    bytes,
                    checksum: (*checksum).to_owned(),
                });
            }
            _ => {
                return Err(WorldError::Qualification(format!(
                    "unrecognized source input line: {line}"
                )));
            }
        }
    }
    if !saw_header || inputs.sealed_target.is_empty() {
        return Err(WorldError::Qualification(
            "source input is incomplete".to_owned(),
        ));
    }
    for member in &mut inputs.members {
        let key = (
            member.domain.clone(),
            member.name.clone(),
            member.version.clone(),
            member.source.clone(),
        );
        if let Some(deps) = pending_deps.remove(&key) {
            member.deps = deps;
        }
        if let Some(files) = pending_files.remove(&(member.domain.clone(), member.source.clone())) {
            member.files = Some(files);
        }
    }
    if !pending_deps.is_empty() || !pending_files.is_empty() {
        return Err(WorldError::Qualification(
            "dependency or path-file row has no member".to_owned(),
        ));
    }
    inputs.members.sort_by(|left, right| {
        (&left.domain, &left.name, &left.version, &left.source).cmp(&(
            &right.domain,
            &right.name,
            &right.version,
            &right.source,
        ))
    });
    Ok(inputs)
}

struct MemberParts<'a> {
    domain: &'a str,
    origin: &'a str,
    name: &'a str,
    version: &'a str,
    source: &'a str,
    checksum: &'a str,
    kind: &'a str,
    closure: &'a str,
    observed: &'a str,
    compiling: bool,
}

fn member_row(parts: MemberParts<'_>) -> Member {
    Member {
        domain: parts.domain.to_owned(),
        origin: parts.origin.to_owned(),
        name: parts.name.to_owned(),
        version: parts.version.to_owned(),
        source: parts.source.to_owned(),
        checksum: parts.checksum.to_owned(),
        kind: parts.kind.to_owned(),
        closure: parts.closure == "closure",
        observed: parts.observed.to_owned(),
        compiling: parts.compiling,
        deps: Vec::new(),
        files: None,
    }
}

fn canonicalize_root(raw: &str) -> String {
    let mut path = raw.replace('\\', "/");
    while path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    if let Some(stripped) = path.strip_suffix("/.") {
        path = stripped.to_owned();
    }
    if path.is_empty() {
        ".".to_owned()
    } else {
        path
    }
}
