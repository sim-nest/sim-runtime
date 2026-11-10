// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Datum projections of a sealed source universe.

use sim_kernel::{Datum, Symbol};

use super::marks::Change;
use super::{Domain, Generated, Inputs, Member, Shown};
use crate::query;

pub(super) struct SealFace<'a> {
    pub(super) counts: [usize; 4],
    pub(super) work: &'a query::HostWork,
}

pub(super) fn seal_datum(
    batch: &str,
    admission: &str,
    refusal: Option<&(&str, String)>,
    digest: &str,
    inputs: &Inputs,
    requested: &str,
    face: SealFace<'_>,
) -> Datum {
    let SealFace { counts, work } = face;
    let mut fields = vec![
        (
            Symbol::new("admission"),
            Datum::String(admission.to_owned()),
        ),
        (Symbol::new("batch"), Datum::String(batch.to_owned())),
        (
            Symbol::new("compiler-opened"),
            Datum::Bool(work.compiler_opened()),
        ),
        (
            Symbol::new("config"),
            Datum::String(format!(
                "resolver {} lock {}",
                inputs
                    .config
                    .get("resolver")
                    .map(String::as_str)
                    .unwrap_or(""),
                inputs.config.get("lock").map(String::as_str).unwrap_or("")
            )),
        ),
        (Symbol::new("digest"), Datum::String(digest.to_owned())),
        (
            Symbol::new("domains"),
            query::count_datum(usize_count(inputs.domains.len())),
        ),
        (Symbol::new("effects"), query::count_datum(work.effects())),
        (
            Symbol::new("features"),
            query::count_datum(usize_count(inputs.features.len())),
        ),
        (
            Symbol::new("generated"),
            query::count_datum(usize_count(inputs.generated.len())),
        ),
        (
            Symbol::new("members"),
            query::count_datum(usize_count(inputs.members.len())),
        ),
        (
            Symbol::new("patches"),
            query::count_datum(usize_count(inputs.patches.len())),
        ),
        (Symbol::new("resident"), Datum::Bool(true)),
        (
            Symbol::new("sealed-target"),
            Datum::String(inputs.sealed_target.clone()),
        ),
        (
            Symbol::new("subprocesses"),
            query::count_datum(work.subprocesses()),
        ),
        (Symbol::new("target"), Datum::String(requested.to_owned())),
        (
            Symbol::new("toolchain"),
            Datum::String(inputs.toolchain.get("release").cloned().unwrap_or_default()),
        ),
        (
            Symbol::new("verified"),
            query::count_datum(usize_count(counts[0])),
        ),
        (
            Symbol::new("present-unverified"),
            query::count_datum(usize_count(counts[1])),
        ),
        (
            Symbol::new("stub-only"),
            query::count_datum(usize_count(counts[2])),
        ),
        (
            Symbol::new("absent"),
            query::count_datum(usize_count(counts[3])),
        ),
    ];
    if let Some((reason, coordinate)) = refusal {
        fields.push((Symbol::new("coordinate"), Datum::String(coordinate.clone())));
        fields.push((Symbol::new("reason"), Datum::String((*reason).to_owned())));
    }
    Datum::Node {
        tag: Symbol::qualified("world", "source-seal-v1"),
        fields,
    }
}

pub(super) fn universe_datum(
    seal: Datum,
    inputs: &Inputs,
    requested: &str,
    shown: &[Shown],
) -> Datum {
    let mut fields = match seal {
        Datum::Node { fields, .. } => fields,
        other => return other,
    };
    fields.push((
        Symbol::new("domain-rows"),
        Datum::Vector(inputs.domains.iter().map(domain_datum).collect()),
    ));
    fields.push((
        Symbol::new("member-rows"),
        Datum::Vector(
            inputs
                .members
                .iter()
                .zip(shown)
                .map(|(member, status)| member_datum(member, status, requested))
                .collect(),
        ),
    ));
    fields.push((
        Symbol::new("generated-rows"),
        Datum::Vector(inputs.generated.iter().map(generated_datum).collect()),
    ));
    fields.push((
        Symbol::new("feature-rows"),
        Datum::Vector(inputs.features.iter().cloned().map(Datum::String).collect()),
    ));
    Datum::Node {
        tag: Symbol::qualified("world", "source-universe-v1"),
        fields,
    }
}

fn domain_datum((id, domain): (&String, &Domain)) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "resolver-domain-v1"),
        fields: vec![
            (
                Symbol::new("aliases"),
                Datum::Vector(domain.aliases.iter().cloned().map(Datum::String).collect()),
            ),
            (
                Symbol::new("canonical"),
                Datum::String(domain.canonical.clone()),
            ),
            (Symbol::new("id"), Datum::String(id.clone())),
            (Symbol::new("lock"), Datum::String(domain.lock.clone())),
            (Symbol::new("role"), Datum::String(domain.role.clone())),
        ],
    }
}

fn member_datum(member: &Member, shown: &Shown, requested: &str) -> Datum {
    let mut fields = vec![
        (Symbol::new("closure"), Datum::Bool(member.closure)),
        (Symbol::new("domain"), Datum::String(member.domain.clone())),
        (Symbol::new("name"), Datum::String(member.name.clone())),
        (Symbol::new("origin"), Datum::String(member.origin.clone())),
        (Symbol::new("source"), Datum::String(member.source.clone())),
        (
            Symbol::new("status"),
            Datum::String(shown.status.to_owned()),
        ),
        (Symbol::new("target"), Datum::String(requested.to_owned())),
        (
            Symbol::new("version"),
            Datum::String(member.version.clone()),
        ),
    ];
    if let Some(checksum) = &shown.checksum {
        fields.push((Symbol::new("checksum"), Datum::String(checksum.clone())));
    }
    if let Some(kind) = &shown.checksum_kind {
        fields.push((Symbol::new("checksum-kind"), Datum::String(kind.clone())));
    }
    if let Some(content) = &shown.content {
        fields.push((Symbol::new("content"), Datum::String(content.clone())));
    }
    if let Some(reason) = &shown.reason {
        fields.push((Symbol::new("reason"), Datum::String(reason.clone())));
    }
    if !member.deps.is_empty() {
        fields.push((
            Symbol::new("dependencies"),
            Datum::Vector(member.deps.iter().cloned().map(Datum::String).collect()),
        ));
    }
    if let Some(files) = shown.files {
        fields.push((Symbol::new("files"), query::count_datum(files)));
    }
    Datum::Node {
        tag: Symbol::qualified("world", "source-member-v1"),
        fields,
    }
}

fn generated_datum(generated: &Generated) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "generated-input-v1"),
        fields: vec![
            (Symbol::new("bytes"), query::count_datum(generated.bytes)),
            (
                Symbol::new("checksum"),
                Datum::String(generated.checksum.clone()),
            ),
            (
                Symbol::new("domain"),
                Datum::String(generated.domain.clone()),
            ),
            (Symbol::new("path"), Datum::String(generated.path.clone())),
        ],
    }
}

pub(super) fn diff_datum(
    before: &str,
    after: &str,
    inputs: &[Change],
    work: &query::HostWork,
) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "source-seal-diff-v1"),
        fields: vec![
            (Symbol::new("after-digest"), Datum::String(after.to_owned())),
            (
                Symbol::new("before-digest"),
                Datum::String(before.to_owned()),
            ),
            (Symbol::new("changed"), Datum::Bool(before != after)),
            (
                Symbol::new("compiler-opened"),
                Datum::Bool(work.compiler_opened()),
            ),
            (Symbol::new("effects"), query::count_datum(work.effects())),
            (
                Symbol::new("inputs"),
                Datum::Vector(inputs.iter().map(change_datum).collect()),
            ),
            (Symbol::new("resident"), Datum::Bool(true)),
            (
                Symbol::new("subprocesses"),
                query::count_datum(work.subprocesses()),
            ),
        ],
    }
}

fn change_datum(change: &Change) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "changed-input-v1"),
        fields: vec![
            (Symbol::new("domain"), Datum::String(change.domain.clone())),
            (Symbol::new("field"), Datum::String(change.field.clone())),
            (Symbol::new("name"), Datum::String(change.name.clone())),
            (Symbol::new("origin"), Datum::String(change.origin.clone())),
            (
                Symbol::new("version"),
                Datum::String(change.version.clone()),
            ),
        ],
    }
}

pub(super) fn gap(reason: &str, batch: &str, work: &query::HostWork) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "source-gap-v1"),
        fields: vec![
            (Symbol::new("batch"), Datum::String(batch.to_owned())),
            (
                Symbol::new("compiler-opened"),
                Datum::Bool(work.compiler_opened()),
            ),
            (Symbol::new("effects"), query::count_datum(work.effects())),
            (Symbol::new("reason"), Datum::String(reason.to_owned())),
            (Symbol::new("resident"), Datum::Bool(true)),
            (
                Symbol::new("subprocesses"),
                query::count_datum(work.subprocesses()),
            ),
        ],
    }
}

fn usize_count(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
