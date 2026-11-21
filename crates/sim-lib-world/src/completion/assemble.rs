// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Final-world evidence catalog for one admitted phase union.
//!
//! The prove command supplies the selected run's release input. A claim
//! enters this catalog only from an owner receipt the evidence catalog
//! has already admitted for this subject. A source-deck label is not
//! that receipt. A claim with no verified admission stays a gap. This
//! module does not derive a seal.

use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::prove::{PhaseUnionAdmission, admitted_phase_union};

use super::{
    CheckerCatalog, ClaimAdmission, EvidenceCatalog, EvidenceSetId, LinuxBoundary,
    ReleaseObligation, SelectedRoadmapId, WorldSealId,
};

/// Reads the prove command's release input and the admitted phase union.
///
/// A journal catalog supplied for this assembly is retained when it names
/// the admitted subject. Claim 17 enters only from that retained receipt.
/// This function does not run a conformance pack. A missing receipt leaves
/// the claim unproved.
///
/// # Errors
///
/// Returns an error when the release input is not a source identity, or
/// the admitted union has no subject or no fresh specimen.
pub(crate) fn catalog_datum(
    release_input: &str,
    manifest: Option<&str>,
) -> Result<Datum, WorldError> {
    super::owner_admit::clear_source_identity();
    super::owner_admit::clear_journal_completion();
    super::owner_admit::clear_owner_claims();
    crate::journal::clear_export();
    let releases = parse_release_input(release_input)?;
    let admitted = admitted_phase_union()?;
    let _export = crate::journal::take_export();
    if let Some(observation) = super::owner_admit::take_supplied_journal()
        && observation.subject == admitted.subject
    {
        super::owner_admit::retain_journal_completion(
            &observation.subject,
            &observation.result,
            observation.catalog,
            observation.admitted,
            observation.checker,
            observation.scope,
            observation.revocation,
        )?;
    }
    if let Some(source) = super::owner_admit::take_supplied_source()
        && source.subject == admitted.subject
    {
        super::owner_admit::retain_source_identity(
            &source.subject,
            &source.result,
            source.catalog,
            source.checker,
            source.scope,
            source.revocation,
        )?;
    }
    super::owner_admit::accept_supplied_owner_claims(&admitted.subject);
    let supplied_fresh = super::fresh_set::take();
    let fresh = super::fresh_set::retain(
        &admitted.fresh_reason,
        supplied_fresh
            .as_ref()
            .map(|(export, change)| (export.as_str(), change.as_str())),
    )?;
    let (admissions, checks) = super::owner_admit::from_owner(&admitted.subject, &releases)?;
    let catalog = fill(&admitted, &releases, &admissions, manifest, fresh)?;
    Ok(render(&catalog, checks.as_ref()))
}

/// Parses one release identity per line.
///
/// A git line is `git+` commit, package, and version. A registry line is
/// `registry:` package and version. Blank lines are skipped.
///
/// # Errors
///
/// Returns an error when a line is not one of those identities, or when
/// a package name is repeated.
pub(crate) fn parse_release_input(text: &str) -> Result<Vec<ReleaseObligation>, WorldError> {
    let mut releases = Vec::new();
    let mut names = std::collections::BTreeSet::new();
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let order = u16::try_from(releases.len())
            .map_err(|_| WorldError::Qualification("release input is too long".to_owned()))?;
        let Some(release) = git_release(line, order).or_else(|| registry_release(line, order))
        else {
            return Err(WorldError::Qualification(format!(
                "release input line {} is not a source identity",
                index + 1
            )));
        };
        if !names.insert(release.name.clone()) {
            return Err(WorldError::Qualification(format!(
                "release input repeats {}",
                release.name
            )));
        }
        releases.push(release);
    }
    Ok(releases)
}

fn fill(
    admitted: &PhaseUnionAdmission,
    releases: &[ReleaseObligation],
    admissions: &[ClaimAdmission],
    manifest: Option<&str>,
    fresh: Datum,
) -> Result<EvidenceCatalog, WorldError> {
    if !is_digest(&admitted.subject) || admitted.replay_body.is_empty() {
        return Err(WorldError::Qualification(
            "admitted phase union has no subject".to_owned(),
        ));
    }
    let replay_digest = digest(&admitted.replay_body)?;
    let source_digest = selected_source_digest(releases, manifest)?;
    let linux = if admitted.linux_selected {
        LinuxBoundary::SelectedUnfinished
    } else {
        LinuxBoundary::Unselected
    };
    Ok(EvidenceCatalog {
        id: EvidenceSetId {
            value: digest(&identity(admitted, &replay_digest, releases, admissions))?,
        },
        roadmap: SelectedRoadmapId {
            value: admitted.roadmap.clone(),
        },
        final_world: WorldSealId {
            value: admitted.subject.clone(),
        },
        source_digest,
        replay_digest,
        admissions: admissions.to_vec(),
        fresh,
        releases: releases.to_vec(),
        linux,
        model_provider_credential: admitted.credential,
    })
}

fn render(catalog: &EvidenceCatalog, checks: Option<&CheckerCatalog>) -> Datum {
    node(vec![
        (
            "admissions",
            strings(catalog.admissions.iter().map(|row| row.claim.to_string())),
        ),
        ("evidence", text(&catalog.id.value)),
        ("fresh", catalog.fresh.clone()),
        ("gaps", strings(open_gaps(catalog, checks))),
        (
            "releases",
            strings(catalog.releases.iter().map(release_token)),
        ),
        ("replay", text(&catalog.replay_digest)),
        ("roadmap", text(&catalog.roadmap.value)),
        ("seal", text("unclaimed")),
        ("source", text(&catalog.source_digest)),
        ("subject", text(&catalog.final_world.value)),
    ])
}

fn open_gaps(catalog: &EvidenceCatalog, checks: Option<&CheckerCatalog>) -> Vec<String> {
    let mut gaps = Vec::new();
    if let Some(reason) = gap_reason(&catalog.fresh) {
        gaps.push(reason);
    }
    if catalog.releases.is_empty() {
        gaps.push("release-missing".to_owned());
    }
    if !source_covered(catalog, checks) {
        for release in &catalog.releases {
            if !release.registry_published {
                gaps.push(format!("unpublished-git-source:{}", release.name));
            }
        }
    }
    if (1..=25).any(|claim| !catalog.admissions.iter().any(|row| row.claim == claim)) {
        gaps.push("claim-unproved".to_owned());
    }
    if catalog.linux == LinuxBoundary::SelectedUnfinished {
        gaps.push("linux-claim".to_owned());
    }
    gaps
}

fn source_covered(catalog: &EvidenceCatalog, checks: Option<&CheckerCatalog>) -> bool {
    let Some(checks) = checks else {
        return false;
    };
    let found: Vec<_> = catalog
        .admissions
        .iter()
        .filter(|row| row.claim == 21)
        .collect();
    if found.len() != 1 {
        return false;
    }
    let expected = super::check::claim_result(21, &catalog.releases);
    super::check::release_receipt(checks, &catalog.final_world, found[0], &expected).is_ok()
}

fn release_token(release: &ReleaseObligation) -> String {
    if release.registry_published {
        format!("registry:{}@{}", release.name, release.version)
    } else {
        release.source_release.clone()
    }
}

fn identity(
    admitted: &PhaseUnionAdmission,
    replay: &str,
    releases: &[ReleaseObligation],
    admissions: &[ClaimAdmission],
) -> String {
    let mut text = String::new();
    text.push_str(&admitted.roadmap);
    text.push('\n');
    text.push_str(&admitted.subject);
    text.push('\n');
    text.push_str(replay);
    text.push('\n');
    text.push_str(&admitted.fresh_reason);
    text.push('\n');
    for release in releases {
        text.push_str(&release_token(release));
        text.push('\n');
    }
    for admission in admissions {
        text.push_str(&admission.claim.to_string());
        text.push('\n');
    }
    text
}

fn git_release(line: &str, order: u16) -> Option<ReleaseObligation> {
    let rest = line.strip_prefix("git+")?;
    let (commit, spec) = rest.split_once(':')?;
    let (name, version) = spec.split_once('@')?;
    if !is_commit(commit) || name.is_empty() || !is_version(version) {
        return None;
    }
    Some(ReleaseObligation {
        name: name.to_owned(),
        registry_published: false,
        source_release: line.to_owned(),
        version: version.to_owned(),
        order,
    })
}

fn registry_release(line: &str, order: u16) -> Option<ReleaseObligation> {
    let rest = line.strip_prefix("registry:")?;
    let (name, version) = rest.split_once('@')?;
    if name.is_empty() || name.contains(':') || !is_version(version) {
        return None;
    }
    Some(ReleaseObligation {
        name: name.to_owned(),
        registry_published: true,
        source_release: String::new(),
        version: version.to_owned(),
        order,
    })
}

fn is_commit(text: &str) -> bool {
    text.len() == 40
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_version(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
}

fn selected_source_digest(
    releases: &[ReleaseObligation],
    manifest: Option<&str>,
) -> Result<String, WorldError> {
    let mut text = String::new();
    for release in releases {
        text.push_str(&release_token(release));
        text.push('\n');
    }
    if let Some(manifest) = manifest {
        text.push_str(manifest.trim_end());
        text.push('\n');
    }
    digest(&text)
}

fn gap_reason(record: &Datum) -> Option<String> {
    if let Datum::Node { tag, fields } = record
        && tag.as_qualified_str() == "world/fresh-set-v1"
    {
        let admitted = fields
            .iter()
            .find(|(symbol, _)| symbol.as_qualified_str() == "admitted")
            .map(|(_, value)| value)?;
        return gap_reason(admitted);
    }
    let Datum::Node { tag, fields } = record else {
        return None;
    };
    if tag.as_qualified_str() != "world/fresh-gap-v1" {
        return None;
    }
    fields.iter().find_map(|(symbol, value)| {
        if symbol.as_qualified_str() != "reason" {
            return None;
        }
        match value {
            Datum::String(reason) if !reason.is_empty() => Some(reason.clone()),
            _ => None,
        }
    })
}

fn digest(text: &str) -> Result<String, WorldError> {
    let id = crate::provider::content_id(Datum::String(text.to_owned()))
        .map_err(WorldError::Projection)?;
    Ok(hex(&id.bytes))
}

fn is_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
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

fn node(fields: Vec<(&str, Datum)>) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "final-catalog-v1"),
        fields: fields
            .into_iter()
            .map(|(name, value)| (Symbol::new(name), value))
            .collect(),
    }
}

fn text(value: &str) -> Datum {
    Datum::String(value.to_owned())
}

fn strings(values: impl IntoIterator<Item = impl Into<String>>) -> Datum {
    Datum::Vector(
        values
            .into_iter()
            .map(|value| Datum::String(value.into()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use sim_kernel::{Datum, NumberLiteral};

    use crate::prove;
    use crate::prove_command::ProveCommandLib;

    const SOURCES: [&str; 8] = [
        "git+fdb39e34ede165c1e261f518e56a338fc5a3ab33:sim-codec-mspdi@0.1.0",
        "git+a9a376f1376a19f153011e29257f9af7465a323f:sim-ledger@0.1.0",
        "git+a9a376f1376a19f153011e29257f9af7465a323f:sim-ledger-store-port@0.1.0",
        "git+bb8279c1bcbfe14aac8abe75f4bb03f14d29480c:sim-lib-doc-site@0.1.0",
        "git+bb8279c1bcbfe14aac8abe75f4bb03f14d29480c:sim-lib-gantt@0.1.0",
        "git+bb8279c1bcbfe14aac8abe75f4bb03f14d29480c:sim-lib-sheet@0.1.0",
        "git+fdb39e34ede165c1e261f518e56a338fc5a3ab33:sim-site-dalux@0.1.0",
        "git+fdb39e34ede165c1e261f518e56a338fc5a3ab33:sim-site-powerproject@0.1.0",
    ];

    #[test]
    fn the_release_parser_reads_the_caller_input() {
        let rows = super::parse_release_input(
            "git+0123456789abcdef0123456789abcdef01234567:sim-example@0.2.0\nregistry:sim-other@1.0\n",
        )
        .expect("input");
        assert_eq!(rows.len(), 2);
        assert!(!rows[0].registry_published);
        assert_eq!(rows[0].order, 0);
        assert!(rows[1].registry_published);
        assert!(rows[1].source_release.is_empty());
        assert_eq!(rows[0].version, "0.2.0");
        assert_eq!(rows[1].version, "1.0");
        assert_eq!(rows[1].name, "sim-other");
        let older = super::parse_release_input("registry:sim-example@0.1.0\n").expect("older");
        let newer = super::parse_release_input("registry:sim-example@0.2.0\n").expect("newer");
        assert_ne!(
            super::selected_source_digest(&older, Some("manifest")).expect("older digest"),
            super::selected_source_digest(&newer, Some("manifest")).expect("newer digest")
        );
        let repeated = super::parse_release_input(
            "git+0123456789abcdef0123456789abcdef01234567:sim-example@0.2.0\ngit+0123456789abcdef0123456789abcdef01234567:sim-example@0.2.0\n",
        );
        assert!(repeated.is_err());
        let malformed = super::parse_release_input("path/local");
        assert!(malformed.is_err());
    }

    #[test]
    fn a_claim_21_row_without_release_cover_keeps_the_source_gap() {
        use super::super::{
            ClaimAdmission, EvidenceCatalog, EvidenceSetId, LinuxBoundary, ReleaseObligation,
            SelectedRoadmapId, WorldSealId,
        };

        let scoped =
            crate::evidence::scoped_release("world-final", "scope/completion").expect("fixture");
        let (_, admitted, checker) = scoped
            .admissions
            .into_iter()
            .find(|(claim, _, _)| *claim == 1)
            .expect("claim 1");
        let catalog = EvidenceCatalog {
            id: EvidenceSetId {
                value: "ab".repeat(32),
            },
            roadmap: SelectedRoadmapId {
                value: "sim-world-portable-union-v1".to_owned(),
            },
            final_world: WorldSealId {
                value: "cd".repeat(32),
            },
            source_digest: "cd".repeat(32),
            replay_digest: "ef".repeat(32),
            admissions: vec![ClaimAdmission {
                claim: 21,
                admitted,
                checker,
            }],
            fresh: crate::fresh::gap_record("construction-specimen"),
            releases: vec![ReleaseObligation {
                name: "sim-example".to_owned(),
                registry_published: false,
                source_release: "git+0123456789abcdef0123456789abcdef01234567:sim-example@0.1.0"
                    .to_owned(),
                version: "0.1.0".to_owned(),
                order: 0,
            }],
            linux: LinuxBoundary::Unselected,
            model_provider_credential: false,
        };
        for checks in [None, Some(super::super::CheckerCatalog::without_evidence())] {
            let gaps = super::open_gaps(&catalog, checks.as_ref());
            assert!(
                gaps.iter()
                    .any(|gap| gap == "unpublished-git-source:sim-example"),
                "{checks:?}"
            );
            assert!(gaps.iter().any(|gap| gap == "construction-specimen"));
            assert!(gaps.iter().any(|gap| gap == "claim-unproved"));
        }
    }

    #[test]
    fn catalog_without_a_release_input_is_usage() {
        let error = ProveCommandLib::new()
            .run(&argv(&["prove", "catalog"]))
            .expect_err("usage");
        assert!(error.to_string().contains("usage"));
    }

    #[test]
    fn the_final_catalog_fills_from_the_admitted_phase_union() {
        let _ = super::super::fresh_set::take();
        let prove_command = ProveCommandLib::new();
        supply_replay();
        let catalog = prove_command
            .run(&argv(&["prove", "catalog", &SOURCES.join("\n")]))
            .expect("catalog");
        let cold = prove::run("cold", None).expect("cold union");
        let subject = string_field(&cold, "prove/cold-v1", "subject");
        let view = string_field(&cold, "prove/cold-v1", "view");
        assert_eq!(
            string_field(&catalog, "world/final-catalog-v1", "subject"),
            subject
        );
        assert_eq!(
            string_field(&catalog, "world/final-catalog-v1", "roadmap"),
            "sim-world-portable-union-v1"
        );
        let releases = super::parse_release_input(&SOURCES.join("\n")).expect("releases");
        let source = super::selected_source_digest(&releases, None).expect("source");
        assert_eq!(
            string_field(&catalog, "world/final-catalog-v1", "source"),
            source
        );
        assert_ne!(source, subject);
        assert_eq!(
            string_field(&catalog, "world/final-catalog-v1", "seal"),
            "unclaimed"
        );
        assert_eq!(
            string_field(&catalog, "world/final-catalog-v1", "replay").len(),
            64
        );
        assert_ne!(
            string_field(&catalog, "world/final-catalog-v1", "replay"),
            subject
        );
        assert_eq!(
            string_field(&catalog, "world/final-catalog-v1", "evidence").len(),
            64
        );
        assert!(view.contains(&format!("subject {subject}")));
        assert!(view.contains("pass phase/28 "));
        assert_eq!(number_field(&cold, "prove/cold-v1", "denominator"), 29);
        assert_eq!(number_field(&cold, "prove/cold-v1", "executed"), 29);
        assert_eq!(list_field(&catalog, "releases"), SOURCES);
        assert_eq!(list_field(&catalog, "admissions"), vec!["17".to_owned()]);
        let gaps = list_field(&catalog, "gaps");
        assert!(gaps.iter().any(|gap| gap == "construction-specimen"));
        assert!(gaps.iter().any(|gap| gap == "claim-unproved"));
        assert!(!gaps.iter().any(|gap| gap == "release-missing"));
        assert!(!gaps.iter().any(|gap| gap.contains("PortableClosed")));
        for source in SOURCES {
            let name = source
                .split(':')
                .nth(1)
                .expect("package")
                .split('@')
                .next()
                .expect("name");
            assert!(
                gaps.iter()
                    .any(|gap| gap == &format!("unpublished-git-source:{name}"))
            );
        }
        let manifest = "[package]\npublish = false\n[dependencies]\nsim-example = { version = \"0.1.0\", git = \"https://example.invalid/sim-example.git\", rev = \"0123456789abcdef0123456789abcdef01234567\" }\n";
        supply_replay();
        let retained = prove_command
            .run(&argv(&["prove", "catalog", &SOURCES.join("\n"), manifest]))
            .expect("retained");
        assert_eq!(
            list_field(&retained, "admissions"),
            list_field(&catalog, "admissions")
        );
        assert_eq!(list_field(&retained, "gaps"), list_field(&catalog, "gaps"));
        assert_eq!(
            string_field(&retained, "world/final-catalog-v1", "seal"),
            "unclaimed"
        );
        let manifest_source =
            super::selected_source_digest(&releases, Some(manifest)).expect("manifest source");
        assert_eq!(
            string_field(&retained, "world/final-catalog-v1", "source"),
            manifest_source
        );
        assert_ne!(manifest_source, source);
        assert_ne!(manifest_source, subject);
        let fresh = field(&catalog, "world/final-catalog-v1", "fresh").expect("fresh");
        assert_eq!(
            string_field(fresh, "world/fresh-gap-v1", "reason"),
            "construction-specimen"
        );
        assert_eq!(
            string_field(fresh, "world/fresh-gap-v1", "seal"),
            "unclaimed"
        );
    }

    fn supply_replay() {
        let subject = prove::admitted_subject().expect("subject");
        let scoped =
            crate::evidence::scoped_release(&subject, "completion/replay").expect("replay");
        let (_, admitted, checker) = scoped
            .admissions
            .into_iter()
            .find(|(claim, _, _)| *claim == 17)
            .expect("claim 17");
        super::super::owner_admit::supply_journal_observation(
            super::super::owner_admit::SuppliedReplay {
                subject,
                result: "completion/claim-17".to_owned(),
                catalog: scoped.catalog,
                admitted,
                checker,
                scope: scoped.scope,
                revocation: sim_conformance_evidence::RevocationStatus::Current,
            },
        );
    }

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().copied().map(str::to_owned).collect()
    }

    fn string_field(value: &Datum, tag: &str, name: &str) -> String {
        match field(value, tag, name) {
            Some(Datum::String(text)) => text.clone(),
            other => panic!("missing {name}: {other:?}"),
        }
    }

    fn number_field(value: &Datum, tag: &str, name: &str) -> u64 {
        match field(value, tag, name) {
            Some(Datum::Number(NumberLiteral { canonical, .. })) => canonical.parse().expect(name),
            other => panic!("missing {name}: {other:?}"),
        }
    }

    fn list_field(value: &Datum, name: &str) -> Vec<String> {
        match field(value, "world/final-catalog-v1", name) {
            Some(Datum::Vector(items)) => items
                .iter()
                .map(|item| match item {
                    Datum::String(text) => text.clone(),
                    other => panic!("bad list {name}: {other:?}"),
                })
                .collect(),
            other => panic!("missing {name}: {other:?}"),
        }
    }

    fn field<'a>(value: &'a Datum, tag: &str, name: &str) -> Option<&'a Datum> {
        let Datum::Node { tag: found, fields } = value else {
            return None;
        };
        if found.as_qualified_str() != tag {
            return None;
        }
        fields
            .iter()
            .find(|(symbol, _)| symbol.as_qualified_str() == name)
            .map(|(_, field)| field)
    }
}
