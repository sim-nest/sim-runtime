// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use sim_kernel::{ContentId, Symbol, datum_content_algorithm};

use super::{BASE, RENAMED, REORDERED, accept, bind_subject, identity, survey};
use crate::WorldError;

#[test]
fn field_and_role_changes_move_identity_and_aliases_do_not() {
    let base = bind_subject("member", BASE, "note-a").expect("base");
    let reordered = bind_subject("member", REORDERED, "note-a").expect("order");
    let diagnosed = bind_subject("member", BASE, "note-b").expect("diagnostic");
    let renamed = bind_subject("member", RENAMED, "note-a").expect("field");
    let owner = bind_subject("owner", BASE, "note-a").expect("role");
    let base_id = identity(&base).expect("id");
    assert_ne!(base.node, reordered.node);
    assert_eq!(identity(&reordered).expect("alias"), base_id);
    assert_ne!(base.diagnostic, diagnosed.diagnostic);
    assert_eq!(identity(&diagnosed).expect("diagnostic"), base_id);
    assert_ne!(identity(&renamed).expect("field"), base_id);
    assert_ne!(identity(&owner).expect("role"), base_id);
    assert!(matches!(
        bind_subject("guest", BASE, "note-a"),
        Err(WorldError::Qualification(text)) if text.contains("wrong-role")
    ));
    let old = ContentId::from_bytes(Symbol::qualified("core", "sha256"), base_id.bytes);
    assert!(matches!(
        accept(&base, &old),
        Err(WorldError::Qualification(text)) if text == "old-byte"
    ));
    let mut forged_bytes = base_id.bytes;
    forged_bytes[0] ^= 0xff;
    let forged = ContentId::from_bytes(datum_content_algorithm(), forged_bytes);
    assert!(matches!(
        accept(&base, &forged),
        Err(WorldError::Qualification(text)) if text == "fabricated-authority"
    ));
    assert_eq!(accept(&base, &base_id).expect("receiver"), base_id);
    let found = survey().expect("survey");
    assert_eq!(found.reason, "normalized");
    assert_eq!(found.alias, "same");
    assert_eq!(found.diagnostic, "same");
    assert_eq!(found.field, "moved");
    assert_eq!(found.role, "moved");
    assert_eq!(found.wrong_role, "refused");
    assert_eq!(found.old_byte, "refused");
    assert_eq!(found.authority, "refused");
    let source = include_str!("bind_normalize.rs");
    assert!(source.contains("content_id()"));
    assert!(source.contains("datum_content_algorithm()"));
    assert!(!source.contains("ValueFingerprint"));
    let record = super::super::run("normalize").expect("command");
    assert_eq!(text(&record, "reason"), "normalized");
    assert_eq!(text(&record, "alias"), "same");
    assert_eq!(text(&record, "field"), "moved");
}

fn text(value: &sim_kernel::Datum, name: &str) -> String {
    let sim_kernel::Datum::Node { fields, .. } = value else {
        return String::new();
    };
    fields
        .iter()
        .find(|(field, _)| field.to_string() == name)
        .and_then(|(_, item)| match item {
            sim_kernel::Datum::String(text) => Some(text.clone()),
            _ => None,
        })
        .unwrap_or_default()
}
