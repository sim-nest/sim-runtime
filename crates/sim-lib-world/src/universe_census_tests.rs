// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Package-shaped source documents whose feature set is empty.

use crate::WorldError;
use crate::seal::inspect_final;

use super::class_of;

const CHECKSUM: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn an_enumerated_empty_feature_set_is_final_and_moves_when_a_read_fact_changes() {
    let exact = class(document(Some(0), false, false, "3"));
    let unread = class(document(Some(0), false, true, "3"));
    let shifted = class(document(Some(0), false, false, "9"));
    assert_eq!(exact.detail, "exact");
    assert_eq!(unread.detail, "unread-policy");
    assert_eq!(shifted.detail, "exact");
    assert_eq!(unread.digest, exact.digest);
    assert_ne!(shifted.digest, exact.digest);
    assert_eq!(exact.digest.len(), 64);
    assert!(exact.digest.bytes().any(|byte| byte != b'0'));
    assert!(!exact.digest.contains("unread-token"));

    let missing = class(document(None, false, false, "3"));
    assert_eq!(missing.detail, "missing-feature");
    assert!(missing.digest.is_empty());

    let mismatch = inspect_final(Some(&document(Some(0), true, false, "3")));
    match mismatch {
        Err(WorldError::Qualification(message)) => {
            assert!(message.contains("feature census"), "{message}");
        }
        Err(error) => panic!("other error: {error}"),
        Ok(_) => panic!("mismatch was admitted"),
    }
}

fn class(text: String) -> super::Class {
    let face = inspect_final(Some(&text)).unwrap_or_else(|error| panic!("{error}"));
    class_of(&face)
}

fn document(census: Option<u64>, feature: bool, unread: bool, resolver: &str) -> String {
    let mut lines = vec![
        "sim-world-inputs-v1".to_owned(),
        "config lock 4".to_owned(),
        format!("config resolver {resolver}"),
        "root-package sim-lib-world".to_owned(),
        "sealed-target x86_64-unknown-linux-gnu".to_owned(),
        "toolchain commit 1".to_owned(),
        "toolchain host x86_64-unknown-linux-gnu".to_owned(),
        "toolchain release 1.96.0".to_owned(),
        "domain resolver/pkg . compiling Cargo.lock".to_owned(),
        "alias resolver/pkg .".to_owned(),
        format!("member resolver/pkg path sim-lib-world 0.1.19 . {CHECKSUM} path-content closure"),
        "deps resolver/pkg sim-lib-world 0.1.19 . sha2".to_owned(),
        "policy kept present".to_owned(),
    ];
    if let Some(count) = census {
        lines.insert(8, format!("feature-census sim-lib-world {count}"));
    }
    if feature {
        lines.insert(8, "feature sim-lib-world default []".to_owned());
    }
    if unread {
        lines.push("policy extra unread-token".to_owned());
    }
    lines.push("reads fact config|resolver".to_owned());
    lines.push("reads policy kept".to_owned());
    lines.join("\n")
}
