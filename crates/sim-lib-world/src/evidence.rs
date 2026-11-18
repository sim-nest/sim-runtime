// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded evidence case.
//!
//! The case calls the published catalog. Grade promotion keeps one key.
//! A wrong domain, revoked support, a missing object, and a storage id
//! used as a semantic key refuse. Telemetry leaves the key unchanged.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use sim_conformance_evidence::{
    CheckArgument, CheckInputClosureId, CheckScopeId, CheckTemplate, CheckedSubjectId,
    CheckerBinding, CheckerReceipt, CheckerReceiptId, EvidenceGrade, IdKind, PolicyId,
    RevocationStatus, SemanticId,
};
use sim_kernel::{Datum, Symbol};
use sim_lib_roadmap_runner::{
    AdmittedEvidence, DiscoveredDefinition, EvidenceClaim, EvidenceDecision, EvidenceReceiptId,
    ProofCatalog, ProofError, ProofLeaf, RetainedCheckerReceipt,
};

use crate::WorldError;
use crate::query;

#[path = "evidence_closure.rs"]
mod closure;

const SOURCE: &str = "\
sim-world-inputs-v1
root-package lantern
domain resolver/lantern . compiling Cargo.lock
member resolver/lantern path lantern 1.0.0 . aaaa1111 path-content closure
generated resolver/lantern src/generated.rs 4 dddd4444
";

const SUBJECT: &str = "subject/lantern";
const SCOPE: &str = "scope/evidence";
const MARKERS: &[&str] = &[
    "bo-private",
    "docs/private",
    "/home/",
    "private-material:",
    "sim-private/",
];

struct Shape {
    subject: CheckedSubjectId,
    scope: CheckScopeId,
    projections: Vec<String>,
    environment: String,
    source_text: String,
    members: Vec<String>,
    domains: Vec<String>,
    policy: PolicyId,
    storage: String,
}

struct Grades {
    bootstrap: CheckerReceiptId,
    reproducible: CheckerReceiptId,
    release: CheckerReceiptId,
}

struct Survey {
    key: &'static str,
    receipt: &'static str,
    telemetry: &'static str,
    domain: String,
    revoked: String,
    missing: String,
    storage: String,
}

/// Runs one evidence case through the published catalog.
///
/// # Errors
/// Returns an error when the record cannot be represented.
pub(crate) fn run(case: &str) -> Result<Datum, WorldError> {
    if leaked(case) {
        return Ok(gap("", "private-leakage"));
    }
    if case != "admit" {
        return Ok(gap(case, "unknown-case"));
    }
    Ok(admitted())
}

/// The sealed source does not change between calls.
///
/// Status samples repeat this case. The first call admits through the
/// published catalog. Later calls return that same record.
fn admitted() -> Datum {
    static CACHED: OnceLock<Datum> = OnceLock::new();
    if let Some(found) = CACHED.get() {
        return found.clone();
    }
    let datum = match cached() {
        Ok(found) => record(&found.survey),
        Err(error) => return gap("admit", &error.to_string()),
    };
    CACHED.get_or_init(|| datum.clone());
    datum
}

/// Release admission the evidence owner has checked for the lantern fixture.
pub(crate) struct OwnerRelease {
    /// Subject the checker receipt names.
    pub(crate) subject: String,
    /// Result text carried by the release-grade receipt.
    pub(crate) result: String,
    /// Evidence `admit_evidence` accepted for that receipt.
    pub(crate) admitted: AdmittedEvidence,
    /// Checker receipt that bound the admission.
    pub(crate) checker: CheckerReceiptId,
    /// Catalog that holds the receipt.
    pub(crate) catalog: ProofCatalog,
    /// Scope recorded on the receipt.
    pub(crate) scope: CheckScopeId,
    /// Revocation state recorded for that checker.
    pub(crate) revocation: RevocationStatus,
}

/// Returns the evidence owner's verified release admission.
///
/// # Errors
/// Returns an error when the owner cannot build that admission.
pub(crate) fn owner_release() -> Result<&'static OwnerRelease, ProofError> {
    Ok(&cached()?.release)
}

struct Checked {
    survey: Survey,
    release: OwnerRelease,
}

fn cached() -> Result<&'static Checked, ProofError> {
    static CACHED: OnceLock<Checked> = OnceLock::new();
    if let Some(found) = CACHED.get() {
        return Ok(found);
    }
    let built = build_checked()?;
    Ok(CACHED.get_or_init(|| built))
}

fn build_checked() -> Result<Checked, ProofError> {
    let workspace = definition(
        "workspace",
        "resolver/lantern",
        "lantern",
        &["resolver/lantern", "path", "lantern", "1.0.0", "."],
        "aaaa1111",
    );
    let generated = definition(
        "generate",
        "resolver/lantern",
        "src/generated.rs",
        &["resolver/lantern", "src/generated.rs"],
        "dddd4444",
    );
    let found = [workspace, generated];
    let shape = shape_from(&found)?;
    let chosen = [&found[0], &found[1]];
    let leaf = ProofLeaf::ArtifactEquality {
        name: "anchor".into(),
        left: b"same".to_vec(),
        right: b"same".to_vec(),
    };
    let template = claim(&shape, sid("receipt/unused")?, "", Vec::new());
    let bound = closure::input_closure(&leaf, &template, &chosen)?;
    let (mut catalog, grades) = catalog(leaf, bound)?;
    let bootstrap = admit(&mut catalog, &shape, &grades.bootstrap, "", Vec::new())?;
    let checked = admit(
        &mut catalog,
        &shape,
        &grades.reproducible,
        "sample-1",
        Vec::new(),
    )?;
    let published = admit(
        &mut catalog,
        &shape,
        &grades.release,
        "sample-2",
        Vec::new(),
    )?;
    let again = admit(
        &mut catalog,
        &shape,
        &grades.reproducible,
        "different-telemetry",
        Vec::new(),
    )?;
    let mut wrong = claim(&shape, grades.bootstrap.clone(), "", Vec::new());
    wrong.domains = vec!["resolver/other".into()];
    let mut missing = claim(&shape, grades.bootstrap.clone(), "", Vec::new());
    missing.members = vec!["missing-member".into()];
    let mut stored = claim(&shape, grades.bootstrap.clone(), "", Vec::new());
    stored.members = vec![shape.storage.clone()];
    let dependent = admit(
        &mut catalog,
        &shape,
        &grades.reproducible,
        "",
        vec![bootstrap.id.clone()],
    )?;
    let revoked = catalog.decide_evidence(&dependent.key, EvidenceGrade::Reproducible, |id| {
        if id == &grades.bootstrap {
            RevocationStatus::Revoked
        } else {
            RevocationStatus::Current
        }
    });
    let survey = Survey {
        key: mark(bootstrap.key == checked.key && checked.key == published.key),
        receipt: mark(bootstrap.id == checked.id),
        telemetry: mark(again.key == checked.key && again.id == checked.id),
        domain: refused(catalog.admit_evidence(wrong, current)),
        revoked: decided(revoked),
        missing: refused(catalog.admit_evidence(missing, current)),
        storage: refused(catalog.admit_evidence(stored, current)),
    };
    Ok(Checked {
        survey,
        release: OwnerRelease {
            subject: SUBJECT.to_owned(),
            result: RELEASE_RESULT.to_owned(),
            admitted: published,
            checker: grades.release,
            catalog,
            scope: shape.scope,
            revocation: RevocationStatus::Current,
        },
    })
}

const RELEASE_RESULT: &str = "result/published";
fn catalog(
    leaf: ProofLeaf,
    bound: CheckInputClosureId,
) -> Result<(ProofCatalog, Grades), ProofError> {
    let binding = checker_binding()?;
    let invocation = binding
        .instantiate(
            sid("code/catalog")?,
            sid("pack/catalog")?,
            sid(SUBJECT)?,
            sid(SCOPE)?,
            bound,
        )
        .map_err(|error| ProofError::Invalid(error.to_string()))?;
    let mut catalog = ProofCatalog::new([leaf])?;
    let mut grades = Vec::new();
    for (grade, provenance, result) in [
        (
            EvidenceGrade::Bootstrap,
            "provenance/bootstrap",
            "result/bootstrap",
        ),
        (
            EvidenceGrade::Reproducible,
            "provenance/independent",
            "result/checked",
        ),
        (
            EvidenceGrade::Release,
            "provenance/published",
            RELEASE_RESULT,
        ),
    ] {
        let receipt = CheckerReceipt::passing(
            &invocation,
            sid(result)?,
            grade,
            sid(provenance)?,
            sid("policy/catalog")?,
            sid("support/checker")?,
            RevocationStatus::Current,
        )
        .map_err(|error| ProofError::Invalid(error.to_string()))?;
        grades.push(receipt.id().clone());
        catalog = catalog.with_checker_receipt(
            RetainedCheckerReceipt::new(binding.clone(), invocation.clone(), receipt),
            RevocationStatus::Current,
        )?;
    }
    let grades = Grades {
        bootstrap: grades.remove(0),
        reproducible: grades.remove(0),
        release: grades.remove(0),
    };
    Ok((catalog, grades))
}

fn admit(
    catalog: &mut ProofCatalog,
    shape: &Shape,
    checker: &CheckerReceiptId,
    telemetry: &str,
    supports: Vec<EvidenceReceiptId>,
) -> Result<AdmittedEvidence, ProofError> {
    catalog.admit_evidence(claim(shape, checker.clone(), telemetry, supports), current)
}

fn claim(
    shape: &Shape,
    checker: CheckerReceiptId,
    telemetry: &str,
    supports: Vec<EvidenceReceiptId>,
) -> EvidenceClaim {
    EvidenceClaim {
        proof: "anchor".into(),
        subject: shape.subject.clone(),
        scope: shape.scope.clone(),
        checker_receipt: checker,
        projections: shape.projections.clone(),
        environment: shape.environment.clone(),
        source_text: shape.source_text.clone(),
        members: shape.members.clone(),
        domains: shape.domains.clone(),
        policies: vec![shape.policy.clone()],
        supports,
        telemetry: telemetry.into(),
        requested_grade: None,
    }
}

fn shape_from(found: &[DiscoveredDefinition]) -> Result<Shape, ProofError> {
    let workspace = &found[0];
    let generated = &found[1];
    Ok(Shape {
        subject: sid(SUBJECT)?,
        scope: sid(SCOPE)?,
        projections: vec!["projection/body".into()],
        environment: "environment/model".into(),
        source_text: SOURCE.into(),
        members: vec![workspace.semantic.clone(), generated.semantic.clone()],
        domains: vec![workspace.domain.clone()],
        policy: sid("policy/catalog")?,
        storage: workspace.storage.clone(),
    })
}

fn definition(
    class: &'static str,
    domain: &str,
    name: &str,
    fields: &[&str],
    storage: &str,
) -> DiscoveredDefinition {
    DiscoveredDefinition {
        class,
        domain: domain.to_owned(),
        name: name.to_owned(),
        semantic: semantic(class, fields),
        storage: storage.to_owned(),
    }
}

/// Length-prefixed row name the published catalog recomputes from `SOURCE`.
fn semantic(class: &str, fields: &[&str]) -> String {
    let mut parts = Vec::with_capacity(fields.len() + 1);
    parts.push(class);
    parts.extend(fields);
    let mut out = String::new();
    for field in parts {
        out.push_str(&field.len().to_string());
        out.push(':');
        out.push_str(field);
        out.push(';');
    }
    out
}

fn checker_binding() -> Result<CheckerBinding, ProofError> {
    checker_binding_for(SCOPE)
}

fn checker_binding_for(scope: &str) -> Result<CheckerBinding, ProofError> {
    CheckerBinding::new(
        "checker/c-evidence".into(),
        sid("owner/test")?,
        "packs::evidence::check".into(),
        vec![sid("pack/catalog")?],
        sid("shape/check-receipt")?,
        sid("revocation/test")?,
        sid("command/validate")?,
        sid("command/docs")?,
        BTreeSet::from([sid(scope)?]),
        CheckTemplate::new(
            "check".into(),
            vec![
                CheckArgument::BindingSlot,
                CheckArgument::SubjectSlot,
                CheckArgument::ScopeSlot,
            ],
            sid("cwd/source")?,
            sid("environment/sealed")?,
            sid("shape/check-result")?,
        )
        .map_err(|error| ProofError::Invalid(error.to_string()))?,
    )
    .map_err(|error| ProofError::Invalid(error.to_string()))
}

fn current(_: &CheckerReceiptId) -> RevocationStatus {
    RevocationStatus::Current
}

#[cfg(test)]
fn fixture_bound(
    leaf: &sim_lib_roadmap_runner::ProofLeaf,
    template: &sim_lib_roadmap_runner::EvidenceClaim,
    selected: &[&sim_lib_roadmap_runner::DiscoveredDefinition],
) -> Result<CheckInputClosureId, sim_lib_roadmap_runner::ProofError> {
    closure::input_closure(leaf, template, selected)
}

#[cfg(test)]
#[path = "evidence_completion.rs"]
mod completion_link;

#[cfg(test)]
pub(crate) use completion_link::{
    scoped_release, scoped_release_results, scoped_release_with_foreign_support,
};

fn refused(result: Result<AdmittedEvidence, ProofError>) -> String {
    match result {
        Err(ProofError::Refused(reason)) => reason.to_owned(),
        Ok(_) => "admitted".to_owned(),
        Err(_) => "other".to_owned(),
    }
}

fn decided(decision: EvidenceDecision) -> String {
    match decision {
        EvidenceDecision::Refuse { reason } | EvidenceDecision::Recheck { reason } => {
            reason.to_owned()
        }
        EvidenceDecision::Reuse { .. } => "reused".to_owned(),
    }
}

fn mark(equal: bool) -> &'static str {
    if equal { "same" } else { "moved" }
}

fn sid<K: IdKind>(value: &str) -> Result<SemanticId<K>, ProofError> {
    SemanticId::from_text(value).map_err(|error| ProofError::Invalid(error.to_string()))
}

fn record(found: &Survey) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "evidence-v1"),
        fields: vec![
            (Symbol::new("case"), string("admit")),
            (Symbol::new("domain"), string(&found.domain)),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("key"), string(found.key)),
            (Symbol::new("missing"), string(&found.missing)),
            (Symbol::new("receipt"), string(found.receipt)),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("revoked"), string(&found.revoked)),
            (Symbol::new("storage"), string(&found.storage)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
            (Symbol::new("telemetry"), string(found.telemetry)),
        ],
    }
}

fn gap(case: &str, reason: &str) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("world", "evidence-gap-v1"),
        fields: vec![
            (Symbol::new("case"), string(shown(case))),
            (Symbol::new("effects"), query::count_datum(0)),
            (Symbol::new("reason"), string(shown(reason))),
            (Symbol::new("resident"), Datum::Bool(true)),
            (Symbol::new("subprocesses"), query::count_datum(0)),
        ],
    }
}

fn leaked(value: &str) -> bool {
    MARKERS.iter().any(|marker| value.contains(marker))
}

fn shown(value: &str) -> String {
    if leaked(value) {
        String::new()
    } else {
        value.to_owned()
    }
}

fn string(value: impl Into<String>) -> Datum {
    Datum::String(value.into())
}
