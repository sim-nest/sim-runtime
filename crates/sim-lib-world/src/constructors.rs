// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Reached semantic constructors and the byte objects they emit.
//!
//! Each selected constructor builds a canonical datum and places one
//! identity in the semantic authority position. A constructor whose
//! emitted bytes differ from that canonical encoding, a byte object
//! that fails verification, or a storage id, compatibility id, or
//! ephemeral fingerprint in that position refuses. The conclusion
//! digest binds the rows that conclusion reads. This module does not
//! derive a completion seal.

use std::collections::BTreeSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use sha2::{Digest, Sha256};
use sim_conformance_core::{IdKind, SemanticId, StorageId};
use sim_kernel::{ContentId, Datum, Symbol};

use crate::WorldError;

/// Reasons the loaded constructors command returns for the selected set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalConstructors {
    /// Every reached constructor is canonical and every byte object verifies.
    pub canonical: String,
    /// One reached constructor emits bytes other than its canonical encoding.
    pub noncanonical: String,
    /// One byte object fails verification.
    pub unverified: String,
    /// One semantic authority position holds a storage id.
    pub storage_id: String,
    /// One semantic authority position holds a compatibility id.
    pub compatibility_id: String,
    /// One semantic authority position holds an ephemeral fingerprint.
    pub fingerprint: String,
    /// Digest of the rows the canonical conclusion reads.
    pub canonical_seal: String,
    /// Digest after one constructor emits non-canonical bytes.
    pub noncanonical_seal: String,
    /// Digest after one byte object fails verification.
    pub unverified_seal: String,
    /// Digest after one storage id occupies semantic authority.
    pub storage_id_seal: String,
    /// Digest after one compatibility id occupies semantic authority.
    pub compatibility_id_seal: String,
    /// Digest after one ephemeral fingerprint occupies semantic authority.
    pub fingerprint_seal: String,
}

/// Runs the selected constructor cases through the loaded constructors command.
///
/// # Errors
///
/// Returns an error when the command cannot run, a case returns a reason
/// other than the one that case exists to show, or two seals collide.
pub fn execute() -> Result<FinalConstructors, WorldError> {
    let command = crate::command::WorldCommandLib::new()?;
    let (canonical, canonical_seal) = sealed(&command, "canonical", "canonical")?;
    let (noncanonical, noncanonical_seal) = sealed(&command, "noncanonical", "noncanonical")?;
    let (unverified, unverified_seal) = sealed(&command, "unverified", "unverified")?;
    let (storage_id, storage_id_seal) = sealed(&command, "storage-id", "storage-id")?;
    let (compatibility_id, compatibility_id_seal) =
        sealed(&command, "compatibility-id", "compatibility-id")?;
    let (fingerprint, fingerprint_seal) = sealed(&command, "fingerprint", "fingerprint")?;
    let seals = [
        canonical_seal.as_str(),
        noncanonical_seal.as_str(),
        unverified_seal.as_str(),
        storage_id_seal.as_str(),
        compatibility_id_seal.as_str(),
        fingerprint_seal.as_str(),
    ];
    let mut unique = BTreeSet::new();
    if seals.iter().any(|seal| !unique.insert(*seal)) {
        return Err(WorldError::Qualification("seal-identity".to_owned()));
    }
    Ok(FinalConstructors {
        canonical,
        noncanonical,
        unverified,
        storage_id,
        compatibility_id,
        fingerprint,
        canonical_seal,
        noncanonical_seal,
        unverified_seal,
        storage_id_seal,
        compatibility_id_seal,
        fingerprint_seal,
    })
}

/// Digest of the rows one existing constructors case reads.
///
/// # Errors
///
/// Returns an error when the case is unknown or the digest is not a seal.
pub fn constructors_digest(case: &str) -> Result<String, WorldError> {
    if !known(case) {
        return Err(WorldError::Qualification(format!(
            "unknown constructors case {case}"
        )));
    }
    let class = classify(case)?;
    if !hex_seal(&class.digest) {
        return Err(WorldError::Qualification(format!("{case} seal")));
    }
    Ok(class.digest)
}

/// Runs one named constructors case.
///
/// # Errors
///
/// Returns an error when a reached constructor cannot be represented.
pub(crate) fn run(case: &str) -> Result<Datum, WorldError> {
    if !known(case) {
        return Ok(node("unknown-case", ""));
    }
    let class = classify(case)?;
    Ok(node(&class.detail, &class.digest))
}

/// Text the conclusion hashes for `case`.
///
/// # Errors
///
/// Returns an error when the case is unknown.
#[cfg(test)]
pub(crate) fn row_report(case: &str) -> Result<String, WorldError> {
    Ok(classify(case)?.report)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct Meaning;

impl IdKind for Meaning {
    const DOMAIN: &'static str = "world/meaning-v1";
}

fn known(case: &str) -> bool {
    matches!(
        case,
        "canonical"
            | "noncanonical"
            | "unverified"
            | "storage-id"
            | "compatibility-id"
            | "fingerprint"
    )
}

struct Reached {
    id: String,
    role: &'static str,
    emitted: Vec<u8>,
    canonical: Vec<u8>,
    retrieved: Vec<u8>,
    authority: String,
    semantic_authority: String,
    semantic_algorithm: String,
    storage_authority: String,
    storage_algorithm: String,
    fingerprint_authority: String,
}

struct Class {
    detail: String,
    digest: String,
    #[cfg(test)]
    report: String,
}

fn classify(case: &str) -> Result<Class, WorldError> {
    let mut rows = selected()?;
    mutate(&mut rows, case)?;
    let (compat_algorithm, compat_authority) = compatibility();
    let mut lines = Vec::with_capacity(rows.len());
    for row in &rows {
        lines.push(format!(
            "{}|role={}|outcome={}|algorithm={}|authority={}",
            row.id,
            row.role,
            outcome(row),
            authority_algorithm(row, &compat_algorithm, &compat_authority),
            row.authority
        ));
    }
    lines.sort();
    let report = lines.join("\n");
    if report.contains("completion/claim-") {
        return Err(WorldError::Qualification("claim label".to_owned()));
    }
    let detail = detail_of(&rows, &compat_authority);
    let digest = hex(Sha256::digest(report.as_bytes()).as_slice());
    Ok(Class {
        detail: detail.to_owned(),
        digest,
        #[cfg(test)]
        report,
    })
}

fn outcome(row: &Reached) -> &'static str {
    if row.role == "bytes" {
        if verifies(&row.canonical, &row.retrieved) {
            "verified"
        } else {
            "unverified"
        }
    } else if row.emitted == row.canonical {
        "canonical"
    } else {
        "noncanonical"
    }
}

fn authority_algorithm(row: &Reached, compat_algorithm: &str, compat_authority: &str) -> String {
    if row.authority == row.semantic_authority {
        row.semantic_algorithm.clone()
    } else if row.authority == row.storage_authority {
        row.storage_algorithm.clone()
    } else if row.authority == compat_authority {
        compat_algorithm.to_owned()
    } else if row.authority == row.fingerprint_authority {
        "ephemeral/default-hasher-v1".to_owned()
    } else {
        "unknown".to_owned()
    }
}

fn detail_of(rows: &[Reached], compat_authority: &str) -> &'static str {
    if rows
        .iter()
        .any(|row| row.role == "constructor" && row.emitted != row.canonical)
    {
        return "noncanonical";
    }
    if rows
        .iter()
        .any(|row| row.role == "bytes" && !verifies(&row.canonical, &row.retrieved))
    {
        return "unverified";
    }
    if rows
        .iter()
        .any(|row| row.authority == row.storage_authority)
    {
        return "storage-id";
    }
    if rows.iter().any(|row| row.authority == compat_authority) {
        return "compatibility-id";
    }
    if rows
        .iter()
        .any(|row| row.authority == row.fingerprint_authority)
    {
        return "fingerprint";
    }
    if rows
        .iter()
        .any(|row| authority_algorithm(row, "", compat_authority) == "unknown")
    {
        return "gap";
    }
    "canonical"
}

fn verifies(canonical: &[u8], retrieved: &[u8]) -> bool {
    StorageId::for_bytes(canonical).verify(retrieved).is_ok()
}

fn selected() -> Result<Vec<Reached>, WorldError> {
    Ok(vec![
        reach_node()?,
        reach_text()?,
        reach_fields()?,
        reach_bytes()?,
    ])
}

fn mutate(rows: &mut [Reached], case: &str) -> Result<(), WorldError> {
    match case {
        "canonical" => Ok(()),
        "noncanonical" => {
            let row = row_mut(rows, "meaning-node")?;
            row.emitted = corrupt(&row.canonical);
            Ok(())
        }
        "unverified" => {
            let row = row_mut(rows, "stored-body")?;
            row.retrieved = corrupt(&row.canonical);
            Ok(())
        }
        "storage-id" => {
            let storage = row_mut(rows, "meaning-text")?.storage_authority.clone();
            row_mut(rows, "meaning-text")?.authority = storage;
            Ok(())
        }
        "compatibility-id" => {
            row_mut(rows, "meaning-fields")?.authority = compatibility().1;
            Ok(())
        }
        "fingerprint" => {
            let fingerprint = row_mut(rows, "stored-body")?.fingerprint_authority.clone();
            row_mut(rows, "stored-body")?.authority = fingerprint;
            Ok(())
        }
        _ => Err(WorldError::Qualification(format!(
            "unknown constructors case {case}"
        ))),
    }
}

fn row_mut<'a>(rows: &'a mut [Reached], id: &str) -> Result<&'a mut Reached, WorldError> {
    rows.iter_mut()
        .find(|row| row.id == id)
        .ok_or_else(|| WorldError::Qualification(format!("missing constructor {id}")))
}

fn reach_node() -> Result<Reached, WorldError> {
    let datum = meaning("member", "node");
    let semantic = SemanticId::<Meaning>::from_datum(&datum).map_err(fail)?;
    reached("meaning-node", "constructor", &datum, &semantic)
}

fn reach_text() -> Result<Reached, WorldError> {
    let via_text = SemanticId::<Meaning>::from_text("text").map_err(fail)?;
    let datum = meaning("value", "text");
    let via_datum = SemanticId::<Meaning>::from_datum(&datum).map_err(fail)?;
    if via_text != via_datum {
        return Err(WorldError::Qualification("text-constructor".to_owned()));
    }
    reached("meaning-text", "constructor", &datum, &via_text)
}

fn reach_fields() -> Result<Reached, WorldError> {
    let fields = vec![(
        Symbol::qualified("conformance", "member"),
        Datum::String("fields".to_owned()),
    )];
    let via_fields = SemanticId::<Meaning>::from_fields(fields.clone()).map_err(fail)?;
    let datum = Datum::Node {
        tag: Symbol::qualified("world", "meaning-v1"),
        fields,
    };
    let via_datum = SemanticId::<Meaning>::from_datum(&datum).map_err(fail)?;
    if via_fields != via_datum {
        return Err(WorldError::Qualification("fields-constructor".to_owned()));
    }
    reached("meaning-fields", "constructor", &datum, &via_fields)
}

fn reach_bytes() -> Result<Reached, WorldError> {
    let datum = meaning("member", "node");
    let semantic = SemanticId::<Meaning>::from_datum(&datum).map_err(fail)?;
    let reached = reached("stored-body", "bytes", &datum, &semantic)?;
    let location = StorageId::for_bytes(&reached.canonical);
    location.verify(&reached.retrieved).map_err(fail)?;
    if location.verify(&corrupt(&reached.canonical)).is_ok() {
        return Err(WorldError::Qualification(
            "storage-corruption-accepted".to_owned(),
        ));
    }
    Ok(reached)
}

fn reached(
    id: &str,
    role: &'static str,
    datum: &Datum,
    semantic: &SemanticId<Meaning>,
) -> Result<Reached, WorldError> {
    let canonical = datum
        .canonical_bytes()
        .map_err(|error| WorldError::Qualification(error.to_string()))?;
    let semantic_algorithm = semantic.content_id().algorithm.as_qualified_str();
    if semantic_algorithm != "core/sha256-datum-v1" || semantic.content_id().bytes.len() != 32 {
        return Err(WorldError::Qualification("semantic-width".to_owned()));
    }
    let semantic_authority = format!("{semantic_algorithm}:{}", hex(&semantic.content_id().bytes));
    let storage = StorageId::for_bytes(&canonical);
    let storage_algorithm = storage.content_id().algorithm.as_qualified_str();
    let storage_authority = format!(
        "{storage_algorithm}:{}",
        hex(storage.content_id().bytes.as_slice())
    );
    let fingerprint_authority = format!("ephemeral/default-hasher-v1:{}", fingerprint(&canonical));
    let (_, compat_authority) = compatibility();
    if semantic_authority == storage_authority
        || semantic_authority == fingerprint_authority
        || semantic_authority == compat_authority
    {
        return Err(WorldError::Qualification("authority-collision".to_owned()));
    }
    Ok(Reached {
        id: id.to_owned(),
        role,
        emitted: canonical.clone(),
        retrieved: canonical.clone(),
        canonical,
        authority: semantic_authority.clone(),
        semantic_authority,
        semantic_algorithm,
        storage_authority,
        storage_algorithm,
        fingerprint_authority,
    })
}

fn meaning(member: &str, value: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "meaning-v1"),
        fields: vec![(
            Symbol::qualified("conformance", member),
            Datum::String(value.to_owned()),
        )],
    }
}

fn compatibility() -> (String, String) {
    let digest: [u8; 32] = Sha256::digest(b"compat/native-v1").into();
    let id = ContentId::from_bytes(Symbol::qualified("compat", "native-v1"), digest);
    let algorithm = id.algorithm.as_qualified_str();
    (algorithm.clone(), format!("{algorithm}:{}", hex(&id.bytes)))
}

fn fingerprint(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn corrupt(bytes: &[u8]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    if let Some(last) = out.last_mut() {
        *last ^= 0xff;
    } else {
        out.push(0xff);
    }
    out
}

fn fail(error: sim_conformance_core::ConformanceError) -> WorldError {
    WorldError::Qualification(error.to_string())
}

fn sealed(
    command: &crate::command::WorldCommandLib,
    case: &str,
    intended: &str,
) -> Result<(String, String), WorldError> {
    let datum = command.run(&["sim".to_owned(), "constructors".to_owned(), case.to_owned()])?;
    let found =
        detail(&datum).ok_or_else(|| WorldError::Qualification(format!("{case} unreadable")))?;
    if found != intended {
        return Err(WorldError::Qualification(format!(
            "{case} returned {found}"
        )));
    }
    if found.contains("completion/claim-") {
        return Err(WorldError::Qualification(format!(
            "{case} carried a claim label"
        )));
    }
    let digest = text(&datum, "world/constructors-v1", "digest").unwrap_or_default();
    if !hex_seal(&digest) {
        return Err(WorldError::Qualification(format!("{case} seal")));
    }
    Ok((found, digest))
}

fn hex_seal(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        && value.bytes().any(|byte| byte != b'0')
}

fn node(detail: &str, digest: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "constructors-v1"),
        fields: vec![
            (Symbol::new("detail"), Datum::String(detail.to_owned())),
            (Symbol::new("digest"), Datum::String(digest.to_owned())),
        ],
    }
}

fn detail(value: &Datum) -> Option<String> {
    text(value, "world/constructors-v1", "detail")
}

fn text(value: &Datum, tag: &str, name: &str) -> Option<String> {
    let Datum::Node { tag: found, fields } = value else {
        return None;
    };
    if found.as_qualified_str() != tag {
        return None;
    }
    fields.iter().find_map(|(field, item)| {
        if field.to_string() != name {
            return None;
        }
        match item {
            Datum::String(text) => Some(text.clone()),
            _ => None,
        }
    })
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{execute, run};
    use crate::command::WorldCommandLib;

    #[test]
    fn selected_constructors_are_canonical_and_each_fault_stands_alone() {
        let canonical = super::row_report("canonical").expect("rows");
        assert!(canonical.contains("role=constructor"), "{canonical}");
        assert!(canonical.contains("role=bytes"), "{canonical}");
        assert!(
            canonical.contains("meaning-node|role=constructor|outcome=canonical"),
            "{canonical}"
        );
        assert!(
            canonical.contains("meaning-text|role=constructor|outcome=canonical"),
            "{canonical}"
        );
        assert!(
            canonical.contains("meaning-fields|role=constructor|outcome=canonical"),
            "{canonical}"
        );
        assert!(
            canonical.contains("stored-body|role=bytes|outcome=verified"),
            "{canonical}"
        );
        assert!(
            canonical.contains("algorithm=core/sha256-datum-v1"),
            "{canonical}"
        );
        assert!(
            !canonical.contains("storage/sha256-bytes-v1"),
            "{canonical}"
        );
        assert!(!canonical.contains("compat/native-v1"), "{canonical}");
        assert!(
            !canonical.contains("ephemeral/default-hasher-v1"),
            "{canonical}"
        );
        assert!(
            super::row_report("noncanonical")
                .expect("noncanonical")
                .contains("meaning-node|role=constructor|outcome=noncanonical")
        );
        assert!(
            super::row_report("unverified")
                .expect("unverified")
                .contains("stored-body|role=bytes|outcome=unverified")
        );
        assert!(
            super::row_report("storage-id")
                .expect("storage")
                .contains("algorithm=storage/sha256-bytes-v1")
        );
        assert!(
            super::row_report("compatibility-id")
                .expect("compatibility")
                .contains("algorithm=compat/native-v1")
        );
        assert!(
            super::row_report("fingerprint")
                .expect("fingerprint")
                .contains("algorithm=ephemeral/default-hasher-v1")
        );
        let found = execute().expect("constructors");
        assert_eq!(found.canonical, "canonical");
        assert_eq!(found.noncanonical, "noncanonical");
        assert_eq!(found.unverified, "unverified");
        assert_eq!(found.storage_id, "storage-id");
        assert_eq!(found.compatibility_id, "compatibility-id");
        assert_eq!(found.fingerprint, "fingerprint");
        assert!(!format!("{found:?}").contains("completion/claim-"));
        let command = WorldCommandLib::new().expect("command");
        for (case, seal) in [
            ("canonical", found.canonical_seal.as_str()),
            ("noncanonical", found.noncanonical_seal.as_str()),
            ("unverified", found.unverified_seal.as_str()),
            ("storage-id", found.storage_id_seal.as_str()),
            ("compatibility-id", found.compatibility_id_seal.as_str()),
            ("fingerprint", found.fingerprint_seal.as_str()),
        ] {
            assert_eq!(
                super::constructors_digest(case).expect(case),
                seal,
                "{case}"
            );
            assert_eq!(seal_of(&command, case), seal, "{case}");
        }
    }

    #[test]
    fn a_claim_label_is_not_a_constructor_case() {
        let found = run("completion/claim-5").expect("label");
        assert_eq!(super::detail(&found).expect("detail"), "unknown-case");
    }

    #[test]
    fn an_identity_roundtrip_is_not_a_constructor_case() {
        let found = run("identity-roundtrip").expect("roundtrip");
        assert_eq!(super::detail(&found).expect("detail"), "unknown-case");
    }

    fn seal_of(command: &WorldCommandLib, case: &str) -> String {
        let datum = command
            .run(&["sim".to_owned(), "constructors".to_owned(), case.to_owned()])
            .expect(case);
        super::text(&datum, "world/constructors-v1", "digest").unwrap_or_default()
    }
}
