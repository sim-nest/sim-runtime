// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Published pack-binding calls for one bind case.

use sim_conformance_packs::{MemorySubject, PackRequest, PackVerdict, find_pack, packs};
use sim_kernel::{ContentId, Symbol};

pub(crate) struct Law {
    pub(crate) detail: String,
    pub(crate) invokes: u64,
}

pub(crate) enum Fault {
    Bound,
    Refused,
}

impl Fault {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Bound => "bound",
            Self::Refused => "bind-refused",
        }
    }
}

const EFFECT_KEYS: &[&str] = &[
    "work.effect-port-is-local-check-port",
    "work.packet-cannot-construct-native-command",
    "work.packet-names-installed-command-id",
    "work.command-bytes-cannot-be-substituted",
    "work.proposals-remain-pure",
    "work.proposals-apply-only-in-disposable-checkout",
    "work.actual-owner-integration-remains-operator",
    "work.formatter-result-observed",
    "work.test-pass-result-observed",
    "work.test-failure-result-observed",
    "work.validation-and-docs-results-observed",
    "work.operation-outcomes-typed",
    "work.actor-grant-handoff-recorded",
    "work.future-world-source-proof-unassumed",
];

pub(crate) fn serve() -> Result<Law, Fault> {
    let binding = binding("checker/c-work")?;
    let left = subject(0x11)?;
    let right = subject(0x22)?;
    if binding == left || binding == right || left == right {
        return Err(Fault::Refused);
    }
    let empty = MemorySubject::default();
    let effects = effect_evidence();
    let mut invokes = 0;
    let pure_left = call(
        &mut invokes,
        packs::work::check(&request(
            "checker/c-work",
            &binding,
            &left,
            "work/packet-pure",
            &empty,
        )),
    );
    let pure_right = call(
        &mut invokes,
        packs::work::check(&request(
            "checker/c-work",
            &binding,
            &right,
            "work/packet-pure",
            &empty,
        )),
    );
    let effect_left = call(
        &mut invokes,
        packs::work::check(&request(
            "checker/c-work",
            &binding,
            &left,
            "work/packet-effects",
            &effects,
        )),
    );
    let effect_right = call(
        &mut invokes,
        packs::work::check(&request(
            "checker/c-work",
            &binding,
            &right,
            "work/packet-effects",
            &effects,
        )),
    );
    if !distinct(&pure_left, &pure_right) || !distinct(&effect_left, &effect_right) {
        return Err(Fault::Refused);
    }
    Ok(law("pure effects distinct same 1", invokes))
}

pub(crate) fn swapped() -> Result<Law, Fault> {
    let binding = binding("checker/c-work")?;
    let left = subject(0x11)?;
    let empty = MemorySubject::default();
    let mut invokes = 0;
    let verdict = call(
        &mut invokes,
        packs::work::check(&request(
            "checker/c-work",
            &left,
            &binding,
            "work/packet-pure",
            &empty,
        )),
    );
    if refused_code(&verdict) != Some("wrong-binding") {
        return Err(Fault::Refused);
    }
    Ok(law("wrong-binding same 1", invokes))
}

pub(crate) fn missing() -> Result<Law, Fault> {
    let binding = binding("checker/c-work")?;
    let left = subject(0x11)?;
    let expected = funded("checker/c-work", "work/native-dispatch")?;
    let empty = MemorySubject::default();
    let mut invokes = 0;
    let absent = call(
        &mut invokes,
        packs::work::check(&request(
            "checker/c-work",
            &binding,
            &left,
            "work/native-dispatch",
            &empty,
        )),
    );
    let unknown = call(
        &mut invokes,
        packs::work::check(&request(
            "checker/c-work",
            &binding,
            &left,
            "work/not-bound",
            &empty,
        )),
    );
    match (unimplemented(&absent), refused_code(&unknown)) {
        (Some(("checker/c-work", "work/native-dispatch", phase)), Some("wrong-scope"))
            if phase == expected =>
        {
            Ok(law("unimplemented wrong-scope same 1", invokes))
        }
        _ => Err(Fault::Refused),
    }
}

pub(crate) fn facet() -> Result<Law, Fault> {
    let binding = binding("checker/c-facet")?;
    let left = subject(0x11)?;
    let expected = funded("checker/c-facet", "facet/landing")?;
    let empty = MemorySubject::default();
    let mut invokes = 0;
    let pure = call(
        &mut invokes,
        packs::facet::check(&request(
            "checker/c-facet",
            &binding,
            &left,
            "facet/pure",
            &empty,
        )),
    );
    let landing = call(
        &mut invokes,
        packs::facet::check(&request(
            "checker/c-facet",
            &binding,
            &left,
            "facet/landing",
            &empty,
        )),
    );
    if !matches!(pure, PackVerdict::Pass { .. }) {
        return Err(Fault::Refused);
    }
    match unimplemented(&landing) {
        Some(("checker/c-facet", "facet/landing", phase)) if phase == expected => {
            Ok(law("pure landing same 1", invokes))
        }
        _ => Err(Fault::Refused),
    }
}

fn law(detail: &str, invokes: u64) -> Law {
    Law {
        detail: detail.to_owned(),
        invokes,
    }
}

fn call(invokes: &mut u64, verdict: PackVerdict) -> PackVerdict {
    *invokes = invokes.saturating_add(1);
    verdict
}

fn distinct(left: &PackVerdict, right: &PackVerdict) -> bool {
    match (left, right) {
        (PackVerdict::Pass { result: left, .. }, PackVerdict::Pass { result: right, .. }) => {
            left != right
        }
        _ => false,
    }
}

fn refused_code(verdict: &PackVerdict) -> Option<&str> {
    match verdict {
        PackVerdict::Refused(failure) => Some(failure.code),
        _ => None,
    }
}

fn unimplemented(verdict: &PackVerdict) -> Option<(&str, &str, &str)> {
    match verdict {
        PackVerdict::UnimplementedPack {
            checker,
            scope,
            funded_phase,
        } => Some((checker, scope.as_str(), funded_phase.as_str())),
        _ => None,
    }
}

fn request<'a>(
    checker: &'a str,
    binding: &'a str,
    subject: &'a str,
    scope: &'a str,
    evidence: &'a MemorySubject,
) -> PackRequest<'a> {
    PackRequest {
        checker,
        binding,
        subject,
        scope,
        evidence,
    }
}

fn binding(checker: &str) -> Result<String, Fault> {
    let spec = find_pack(checker).ok_or(Fault::Bound)?;
    let bound = spec.checker_binding().map_err(|_| Fault::Bound)?;
    Ok(render(bound.id().content_id()))
}

fn funded(checker: &str, scope: &str) -> Result<String, Fault> {
    let spec = find_pack(checker).ok_or(Fault::Bound)?;
    Ok(spec.funded_phase(scope))
}

fn subject(byte: u8) -> Result<String, Fault> {
    Ok(render(&ContentId::from_bytes(
        Symbol::qualified("core", "sha256-datum-v1"),
        [byte; 32],
    )))
}

fn render(id: &ContentId) -> String {
    let digest: String = id.bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}:{digest}", id.algorithm.as_qualified_str())
}

fn effect_evidence() -> MemorySubject {
    EFFECT_KEYS
        .iter()
        .fold(MemorySubject::default(), |subject, key| {
            subject.with(*key, "true")
        })
}
