// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Model delivery and operator release compared with independent observations.
//!
//! Model delivery rows come from the delivery catalog. Each row is compared
//! with a service snapshot taken beside that catalog. Operator release rows
//! come from the release plan and are compared with a service snapshot of
//! the real realization. A row from one realization is refused when it is
//! offered as the observation of the other. The conclusion digest binds the
//! rows that conclusion reads. This module does not derive a completion seal.

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};
use sim_kernel::{Datum, Symbol};

use crate::WorldError;
use crate::{deliver, release, service};

/// Reasons the loaded domains command returns for the selected set.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalDomains {
    /// Model delivery facts equal independent model observations.
    pub model: String,
    /// Operator release facts equal independent real observations.
    pub real: String,
    /// Neither realization's facts stand in for the other realization.
    pub substitution: String,
    /// Digest of the rows the model conclusion reads.
    pub model_seal: String,
    /// Digest of the rows the real conclusion reads.
    pub real_seal: String,
    /// Digest of the rows the substitution conclusion reads.
    pub substitution_seal: String,
}

/// Runs the selected domain cases through the loaded domains command.
///
/// # Errors
///
/// Returns an error when the command cannot run, a case returns a reason
/// other than the one that case exists to show, or two seals collide.
pub fn execute() -> Result<FinalDomains, WorldError> {
    let command = crate::command::WorldCommandLib::new()?;
    let (model, model_seal) = sealed(&command, "model", "aligned")?;
    let (real, real_seal) = sealed(&command, "real", "aligned")?;
    let (substitution, substitution_seal) = sealed(&command, "substituted", "refused")?;
    let seals = [
        model_seal.as_str(),
        real_seal.as_str(),
        substitution_seal.as_str(),
    ];
    let mut unique = BTreeSet::new();
    if seals.iter().any(|seal| !unique.insert(*seal)) {
        return Err(WorldError::Qualification("seal-identity".to_owned()));
    }
    Ok(FinalDomains {
        model,
        real,
        substitution,
        model_seal,
        real_seal,
        substitution_seal,
    })
}

/// Runs one named domains case.
///
/// # Errors
///
/// Returns an error when a catalog or a snapshot cannot be read.
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
/// Returns an error when the case is unknown or a catalog cannot be read.
#[cfg(test)]
pub(crate) fn row_report(case: &str) -> Result<String, WorldError> {
    if !known(case) {
        return Err(WorldError::Qualification(format!(
            "unknown domains case {case}"
        )));
    }
    Ok(classify(case)?.report)
}

fn known(case: &str) -> bool {
    matches!(case, "model" | "real" | "substituted")
}

struct Class {
    detail: String,
    digest: String,
    #[cfg(test)]
    report: String,
}

fn classify(case: &str) -> Result<Class, WorldError> {
    let (detail, report) = match case {
        "model" => model_case()?,
        "real" => real_case()?,
        "substituted" => substituted_case()?,
        _ => {
            return Err(WorldError::Qualification(format!(
                "unknown domains case {case}"
            )));
        }
    };
    if report.contains("completion/claim-") {
        return Err(WorldError::Qualification("claim label".to_owned()));
    }
    Ok(Class {
        detail,
        digest: hex(Sha256::digest(report.as_bytes()).as_slice()),
        #[cfg(test)]
        report,
    })
}

fn model_case() -> Result<(String, String), WorldError> {
    let selectors = selectors();
    let before = capture("model", &selectors)?;
    let plan = deliver::catalog("model")?;
    let after = capture("model", &selectors)?;
    let rows = plan_rows(
        &plan,
        "world/deliver-v1",
        &[
            ("actions", "world/deliver-action-v1"),
            ("omitted", "world/deliver-omission-v1"),
        ],
    )?;
    let independent = before == after;
    let detail = if independent && rows_match(&rows, &before) {
        "aligned"
    } else {
        "model-diverged"
    };
    Ok((
        detail.to_owned(),
        render("model", independent, &rows, &before),
    ))
}

fn real_case() -> Result<(String, String), WorldError> {
    let selectors = selectors();
    let before = capture("real", &selectors)?;
    let plan = release::plan_catalog("real")?;
    let after = capture("real", &selectors)?;
    let rows = plan_rows(
        &plan,
        "world/release-plan-v1",
        &[("actions", "world/release-action-v1")],
    )?;
    let omitted = field_len(&plan, "world/release-plan-v1", "omitted")?;
    let independent = before == after;
    let detail = if independent && omitted == 0 && rows_match(&rows, &before) {
        "aligned"
    } else {
        "real-diverged"
    };
    let mut report = render("real", independent, &rows, &before);
    report.push('\n');
    report.push_str(&format!("omitted={omitted}"));
    Ok((detail.to_owned(), report))
}

fn substituted_case() -> Result<(String, String), WorldError> {
    let selectors = selectors();
    let model_observations = capture("model", &selectors)?;
    let real_observations = capture("real", &selectors)?;
    let model_rows = plan_rows(
        &deliver::catalog("model")?,
        "world/deliver-v1",
        &[
            ("actions", "world/deliver-action-v1"),
            ("omitted", "world/deliver-omission-v1"),
        ],
    )?;
    let real_plan = release::plan_catalog("real")?;
    let real_rows = plan_rows(
        &real_plan,
        "world/release-plan-v1",
        &[("actions", "world/release-action-v1")],
    )?;
    let omitted = field_len(&real_plan, "world/release-plan-v1", "omitted")?;
    let refused = omitted == 0
        && cross_refused(&model_rows, &real_observations)
        && cross_refused(&real_rows, &model_observations);
    let detail = if refused { "refused" } else { "substituted" };
    let mut report = render("model-as-real", true, &model_rows, &real_observations);
    report.push('\n');
    report.push_str(&render(
        "real-as-model",
        true,
        &real_rows,
        &model_observations,
    ));
    report.push('\n');
    report.push_str(&format!("omitted={omitted}"));
    Ok((detail.to_owned(), report))
}

fn selectors() -> Vec<String> {
    let mut selectors: Vec<String> = service::construction_rows()
        .map(|(_, selector, _, _)| selector.to_owned())
        .collect();
    selectors.sort();
    selectors.dedup();
    selectors
}

fn capture(
    realization: &str,
    selectors: &[String],
) -> Result<BTreeMap<String, Observed>, WorldError> {
    let mut observations = BTreeMap::new();
    for selector in selectors {
        let snapshot = service::snapshot(realization, selector)?;
        observations.insert(selector.clone(), observation(&snapshot, selector)?);
    }
    Ok(observations)
}

fn rows_match(rows: &[Observed], observations: &BTreeMap<String, Observed>) -> bool {
    !rows.is_empty()
        && rows
            .iter()
            .all(|row| observations.get(&row.selector) == Some(row))
}

fn cross_refused(rows: &[Observed], observations: &BTreeMap<String, Observed>) -> bool {
    let mut pairs = 0;
    for row in rows {
        let Some(other) = observations.get(&row.selector) else {
            continue;
        };
        if row == other {
            return false;
        }
        pairs += 1;
    }
    pairs > 0
}

fn render(
    domain: &str,
    independent: bool,
    rows: &[Observed],
    observations: &BTreeMap<String, Observed>,
) -> String {
    let mut lines = vec![
        format!("domain={domain}"),
        format!("independent={}", if independent { "yes" } else { "no" }),
        format!("rows={}", rows.len()),
    ];
    for row in rows {
        let other = observations.get(&row.selector);
        lines.push(format!(
            "selector={} left-state={} right-state={} left-value={} right-value={} left-provenance={} right-provenance={}",
            row.selector,
            row.state,
            other.map(|item| item.state.as_str()).unwrap_or("-"),
            shown(row.value.as_deref()),
            other
                .map(|item| shown(item.value.as_deref()))
                .unwrap_or_else(|| "-".to_owned()),
            row.provenance,
            other.map(|item| item.provenance.as_str()).unwrap_or("-"),
        ));
    }
    lines.sort();
    lines.join("\n")
}

fn shown(value: Option<&str>) -> String {
    value.unwrap_or("-").to_owned()
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Observed {
    selector: String,
    state: String,
    value: Option<String>,
    provenance: String,
}

fn observation(snapshot: &Datum, selector: &str) -> Result<Observed, WorldError> {
    let fields = node_fields(snapshot, "world/service-snapshot-v1")?;
    for item in vector_field(fields, "observations")? {
        let row = node_fields(&item, "world/service-observation-v1")?;
        if required(row, "selector")? == selector {
            return observed(row);
        }
    }
    Err(WorldError::Qualification(format!(
        "snapshot missed {selector}"
    )))
}

fn plan_rows(
    plan: &Datum,
    tag: &str,
    groups: &[(&str, &str)],
) -> Result<Vec<Observed>, WorldError> {
    let fields = node_fields(plan, tag)?;
    let mut rows = Vec::new();
    for (name, row_tag) in groups {
        for item in vector_field(fields, name)? {
            rows.push(observed(node_fields(&item, row_tag)?)?);
        }
    }
    rows.sort_by(|left, right| left.selector.cmp(&right.selector));
    Ok(rows)
}

fn observed(fields: &[(Symbol, Datum)]) -> Result<Observed, WorldError> {
    Ok(Observed {
        selector: required(fields, "selector")?,
        state: required(fields, "state")?,
        value: optional(fields, "value")?,
        provenance: required(fields, "provenance")?,
    })
}

fn field_len(plan: &Datum, tag: &str, name: &str) -> Result<usize, WorldError> {
    let fields = node_fields(plan, tag)?;
    Ok(vector_field(fields, name)?.len())
}

fn node_fields<'a>(value: &'a Datum, tag: &str) -> Result<&'a [(Symbol, Datum)], WorldError> {
    match value {
        Datum::Node {
            tag: actual,
            fields,
        } if actual.as_qualified_str() == tag => Ok(fields),
        _ => Err(WorldError::Qualification(format!("expected {tag}"))),
    }
}

fn vector_field(fields: &[(Symbol, Datum)], name: &str) -> Result<Vec<Datum>, WorldError> {
    match field(fields, name)? {
        Datum::Vector(items) => Ok(items),
        _ => Err(WorldError::Qualification(format!("{name} is not a vector"))),
    }
}

fn required(fields: &[(Symbol, Datum)], name: &str) -> Result<String, WorldError> {
    match field(fields, name)? {
        Datum::String(value) => Ok(value),
        _ => Err(WorldError::Qualification(format!("{name} is not text"))),
    }
}

fn optional(fields: &[(Symbol, Datum)], name: &str) -> Result<Option<String>, WorldError> {
    match fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, item)| item)
    {
        Some(Datum::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(WorldError::Qualification(format!("{name} is not text"))),
        None => Ok(None),
    }
}

fn field(fields: &[(Symbol, Datum)], name: &str) -> Result<Datum, WorldError> {
    fields
        .iter()
        .find(|(symbol, _)| symbol.as_qualified_str() == name)
        .map(|(_, item)| item.clone())
        .ok_or_else(|| WorldError::Qualification(format!("{name} is absent")))
}

fn sealed(
    command: &crate::command::WorldCommandLib,
    case: &str,
    intended: &str,
) -> Result<(String, String), WorldError> {
    let datum = command.run(&["sim".to_owned(), "domains".to_owned(), case.to_owned()])?;
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
    let digest = text(&datum, "world/domains-v1", "digest").unwrap_or_default();
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
        tag: Symbol::qualified("world", "domains-v1"),
        fields: vec![
            (Symbol::new("detail"), Datum::String(detail.to_owned())),
            (Symbol::new("digest"), Datum::String(digest.to_owned())),
        ],
    }
}

fn detail(value: &Datum) -> Option<String> {
    text(value, "world/domains-v1", "detail")
}

fn text(value: &Datum, tag: &str, name: &str) -> Option<String> {
    let Datum::Node { tag: found, fields } = value else {
        return None;
    };
    if found.as_qualified_str() != tag {
        return None;
    }
    fields.iter().find_map(|(field, item)| {
        if field.as_qualified_str() != name {
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
    fn delivery_and_release_match_independent_observations() {
        let model_plan = crate::deliver::catalog("model").expect("deliver");
        let model_rows = super::plan_rows(
            &model_plan,
            "world/deliver-v1",
            &[
                ("actions", "world/deliver-action-v1"),
                ("omitted", "world/deliver-omission-v1"),
            ],
        )
        .expect("model rows");
        assert!(!model_rows.is_empty());
        let model_snapshot =
            crate::service::snapshot("model", &model_rows[0].selector).expect("model snapshot");
        let model_observation = super::observation(&model_snapshot, &model_rows[0].selector)
            .expect("model observation");
        assert_eq!(model_observation, model_rows[0]);
        let model_report = super::row_report("model").expect("model report");
        assert!(
            model_report.contains(&model_observation.provenance),
            "{model_report}"
        );

        let real_plan = crate::release::plan_catalog("real").expect("release");
        let real_rows = super::plan_rows(
            &real_plan,
            "world/release-plan-v1",
            &[("actions", "world/release-action-v1")],
        )
        .expect("real rows");
        assert!(!real_rows.is_empty());
        let real_snapshot =
            crate::service::snapshot("real", &real_rows[0].selector).expect("real snapshot");
        let real_observation =
            super::observation(&real_snapshot, &real_rows[0].selector).expect("real observation");
        assert_eq!(real_observation, real_rows[0]);
        assert_ne!(model_observation, real_observation);
        let real_report = super::row_report("real").expect("real report");
        assert!(
            real_report.contains(&real_observation.provenance),
            "{real_report}"
        );
        let crossed = super::row_report("substituted").expect("crossed");
        assert!(crossed.contains(&model_observation.provenance), "{crossed}");
        assert!(crossed.contains(&real_observation.provenance), "{crossed}");
        assert!(!crossed.contains("completion/claim-"));

        let found = execute().expect("domains");
        assert_eq!(found.model, "aligned");
        assert_eq!(found.real, "aligned");
        assert_eq!(found.substitution, "refused");
        assert!(!format!("{found:?}").contains("completion/claim-"));
        let command = WorldCommandLib::new().expect("command");
        for (case, seal) in [
            ("model", found.model_seal.as_str()),
            ("real", found.real_seal.as_str()),
            ("substituted", found.substitution_seal.as_str()),
        ] {
            assert_eq!(seal_of(&command, case), seal, "{case}");
        }
    }

    #[test]
    fn a_claim_label_is_not_a_domain_case() {
        let found = run("completion/claim-14").expect("label");
        assert_eq!(super::detail(&found).expect("detail"), "unknown-case");
    }

    fn seal_of(command: &WorldCommandLib, case: &str) -> String {
        let datum = command
            .run(&["sim".to_owned(), "domains".to_owned(), case.to_owned()])
            .expect(case);
        super::text(&datum, "world/domains-v1", "digest").unwrap_or_default()
    }
}
