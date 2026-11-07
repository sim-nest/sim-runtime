// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Field-level identity of one sealed source universe.

use std::collections::BTreeMap;

use super::Inputs;

pub(super) struct Change {
    pub(super) domain: String,
    pub(super) origin: String,
    pub(super) name: String,
    pub(super) version: String,
    pub(super) field: String,
}

pub(super) fn field_marks(inputs: &Inputs) -> BTreeMap<String, String> {
    let mut marks = BTreeMap::new();
    for (key, value) in &inputs.config {
        marks.insert(format!("config|{key}"), value.clone());
    }
    for (key, value) in &inputs.toolchain {
        marks.insert(format!("toolchain|{key}"), value.clone());
    }
    marks.insert("sealed-target".to_owned(), inputs.sealed_target.clone());
    for feature in &inputs.features {
        let mut parts = feature.splitn(3, ' ');
        let package = parts.next().unwrap_or("");
        let name = parts.next().unwrap_or("");
        let body = parts.next().unwrap_or("");
        marks.insert(format!("feature|{package}|{name}"), body.to_owned());
    }
    for activate in &inputs.activates {
        marks.insert(format!("activate|{activate}"), "present".to_owned());
    }
    for patch in &inputs.patches {
        marks.insert(format!("patch|{patch}"), "present".to_owned());
    }
    for (id, domain) in &inputs.domains {
        marks.insert(format!("domain|{id}|canonical"), domain.canonical.clone());
        marks.insert(format!("domain|{id}|role"), domain.role.clone());
        marks.insert(format!("domain|{id}|lock"), domain.lock.clone());
        let aliases = domain.aliases.iter().cloned().collect::<Vec<_>>().join(",");
        marks.insert(format!("alias|{id}"), aliases);
    }
    for member in &inputs.members {
        let prefix = format!(
            "member|{}|{}|{}|{}|{}",
            member.domain, member.origin, member.name, member.version, member.source
        );
        marks.insert(format!("{prefix}|checksum"), member.checksum.clone());
        marks.insert(format!("{prefix}|kind"), member.kind.clone());
        marks.insert(
            format!("{prefix}|closure"),
            if member.closure { "closure" } else { "outside" }.to_owned(),
        );
        let mut deps = member.deps.clone();
        deps.sort();
        marks.insert(format!("{prefix}|dependencies"), deps.join("\t"));
        if let Some(files) = member.files {
            marks.insert(format!("{prefix}|files"), files.to_string());
        }
    }
    for generated in &inputs.generated {
        let prefix = format!("generated|{}|{}", generated.domain, generated.path);
        marks.insert(format!("{prefix}|bytes"), generated.bytes.to_string());
        marks.insert(format!("{prefix}|checksum"), generated.checksum.clone());
    }
    marks
}

pub(super) fn digest_body(marks: &BTreeMap<String, String>) -> String {
    marks
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn changed(
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
) -> Vec<Change> {
    let mut keys = BTreeMap::new();
    for key in before.keys().chain(after.keys()) {
        keys.insert(key.clone(), ());
    }
    let mut changes = Vec::new();
    for key in keys.into_keys() {
        if before.get(&key) != after.get(&key) {
            changes.push(change_from_key(&key));
        }
    }
    changes
}

fn change_from_key(key: &str) -> Change {
    let parts: Vec<&str> = key.split('|').collect();
    match parts.as_slice() {
        ["member", domain, origin, name, version, _source, field] => Change {
            domain: (*domain).to_owned(),
            origin: (*origin).to_owned(),
            name: (*name).to_owned(),
            version: (*version).to_owned(),
            field: (*field).to_owned(),
        },
        ["toolchain", name] => named("toolchain", name, "toolchain"),
        ["config", name] => named("config", name, "config"),
        ["feature", package, name] => Change {
            domain: (*package).to_owned(),
            origin: "feature".to_owned(),
            name: (*name).to_owned(),
            version: String::new(),
            field: "feature".to_owned(),
        },
        ["activate", name] => named("activate", name, "activate"),
        ["patch", name] => named("patch", name, "patch"),
        ["domain", id, field] => Change {
            domain: (*id).to_owned(),
            origin: "domain".to_owned(),
            name: (*id).to_owned(),
            version: String::new(),
            field: (*field).to_owned(),
        },
        ["alias", id] => Change {
            domain: (*id).to_owned(),
            origin: "alias".to_owned(),
            name: (*id).to_owned(),
            version: String::new(),
            field: "alias".to_owned(),
        },
        ["generated", domain, path, field] => Change {
            domain: (*domain).to_owned(),
            origin: "generated".to_owned(),
            name: (*path).to_owned(),
            version: String::new(),
            field: (*field).to_owned(),
        },
        ["sealed-target"] => named("target", "sealed-target", "sealed-target"),
        _ => named("", key, "input"),
    }
}

fn named(origin: &str, name: &str, field: &str) -> Change {
    Change {
        domain: String::new(),
        origin: origin.to_owned(),
        name: name.to_owned(),
        version: String::new(),
        field: field.to_owned(),
    }
}
