// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Binding identity for one world bind case.
//!
//! One binding serves two subjects and two funded scopes. A swapped
//! identity refuses. A known unfunded scope stays unimplemented, and an
//! unknown scope refuses. The funded facet scope passes. This module does
//! not call a conformance pack.

use sim_kernel::{ContentId, Datum, Symbol};

pub(crate) struct Law {
    pub(crate) detail: String,
    pub(crate) invokes: u64,
}

pub(crate) enum Fault {
    Bound,
    Refused,
}

impl Fault {
    pub(crate) const fn code(self) -> &'static str {
        match self {
            Self::Bound => "bound",
            Self::Refused => "bind-refused",
        }
    }
}

const PURE: &str = "world/packet-pure";
const EFFECTS: &str = "world/packet-effects";
const NATIVE: &str = "world/native-dispatch";
const UNKNOWN: &str = "world/not-bound";
const FACET_PURE: &str = "world/facet-pure";
const FACET_LANDING: &str = "world/facet-landing";

pub(crate) fn serve() -> Result<Law, Fault> {
    let binding = binding_id()?;
    let left = subject_id(0x11)?;
    let right = subject_id(0x22)?;
    if binding == left || binding == right || left == right {
        return Err(Fault::Refused);
    }
    let mut invokes = 0;
    let pure_left = call(&mut invokes, &binding, &left, PURE)?;
    let pure_right = call(&mut invokes, &binding, &right, PURE)?;
    let effect_left = call(&mut invokes, &binding, &left, EFFECTS)?;
    let effect_right = call(&mut invokes, &binding, &right, EFFECTS)?;
    if pure_left == pure_right || effect_left == effect_right || pure_left == effect_left {
        return Err(Fault::Refused);
    }
    let repeated = receipt(&binding, &left, PURE)?;
    let same = u64::from(repeated == pure_left);
    if same != 1 {
        return Err(Fault::Refused);
    }
    Ok(law(&format!("pure effects distinct same {same}"), invokes))
}

pub(crate) fn swapped() -> Result<Law, Fault> {
    let binding = binding_id()?;
    let left = subject_id(0x11)?;
    let mut invokes = 0;
    let reason = presented(&mut invokes, &left, &binding);
    if reason != "wrong-binding" {
        return Err(Fault::Refused);
    }
    let again = if left == binding {
        "accepted"
    } else {
        "wrong-binding"
    };
    let same = u64::from(reason == again);
    Ok(law(&format!("{reason} same {same}"), invokes))
}

pub(crate) fn missing() -> Result<Law, Fault> {
    let mut invokes = 0;
    let absent = classify(&mut invokes, NATIVE);
    let unknown = classify(&mut invokes, UNKNOWN);
    if absent == "unimplemented" && unknown == "wrong-scope" {
        let same = u64::from(scope_class(NATIVE) == absent && scope_class(UNKNOWN) == unknown);
        return Ok(law(&format!("{absent} {unknown} same {same}"), invokes));
    }
    Err(Fault::Refused)
}

pub(crate) fn crossed() -> Result<Law, Fault> {
    let producer = party("binding", "producer")?;
    let receiver = party("binding", "receiver")?;
    classified(&producer, &receiver, "wrong-binding")
}

pub(crate) fn confused() -> Result<Law, Fault> {
    let producer = party("binding", "producer")?;
    let foreign = party("model", "producer")?;
    classified(&producer, &foreign, "domain-confusion")
}

pub(crate) fn mutated() -> Result<Law, Fault> {
    let producer = party("binding", "producer")?;
    let mut bytes = producer.id.bytes;
    bytes[0] ^= 0xff;
    let mutated = Party {
        kind: producer.kind,
        id: ContentId::from_bytes(producer.id.algorithm.clone(), bytes),
    };
    classified(&producer, &mutated, "wrong-binding")
}

pub(crate) fn exact() -> Result<Law, Fault> {
    let producer = party("binding", "producer")?;
    classified(&producer, &producer, "accepted")
}

fn classified(expected: &Party, presented: &Party, intended: &str) -> Result<Law, Fault> {
    let reason = presentation_reason(expected, presented);
    if reason != intended {
        return Err(Fault::Refused);
    }
    Ok(law(reason, 1))
}

pub(crate) fn facet() -> Result<Law, Fault> {
    let binding = binding_id()?;
    let left = subject_id(0x11)?;
    let mut invokes = 0;
    let pure = call(&mut invokes, &binding, &left, FACET_PURE)?;
    let landing = classify(&mut invokes, FACET_LANDING);
    if landing != "unimplemented" {
        return Err(Fault::Refused);
    }
    let same = u64::from(receipt(&binding, &left, FACET_PURE)? == pure);
    if same != 1 {
        return Err(Fault::Refused);
    }
    Ok(law(&format!("pure landing same {same}"), invokes))
}

fn law(detail: &str, invokes: u64) -> Law {
    Law {
        detail: detail.to_owned(),
        invokes,
    }
}

fn call(
    invokes: &mut u64,
    binding: &ContentId,
    subject: &ContentId,
    scope: &str,
) -> Result<ContentId, Fault> {
    *invokes = invokes.saturating_add(1);
    if funded_phase(scope).is_none() {
        return Err(Fault::Refused);
    }
    receipt(binding, subject, scope)
}

fn presented(
    invokes: &mut u64,
    presented_binding: &ContentId,
    binding: &ContentId,
) -> &'static str {
    *invokes = invokes.saturating_add(1);
    if presented_binding == binding {
        "accepted"
    } else {
        "wrong-binding"
    }
}

fn classify(invokes: &mut u64, scope: &str) -> &'static str {
    *invokes = invokes.saturating_add(1);
    scope_class(scope)
}

fn scope_class(scope: &str) -> &'static str {
    if funded_phase(scope).is_some() {
        "funded"
    } else if unfunded(scope) {
        "unimplemented"
    } else {
        "wrong-scope"
    }
}

fn funded_phase(scope: &str) -> Option<&'static str> {
    match scope {
        PURE | EFFECTS => Some("world/bind"),
        FACET_PURE => Some("world/facet"),
        _ => None,
    }
}

fn unfunded(scope: &str) -> bool {
    matches!(scope, NATIVE | FACET_LANDING)
}

fn receipt(binding: &ContentId, subject: &ContentId, scope: &str) -> Result<ContentId, Fault> {
    let node = Datum::Node {
        tag: Symbol::qualified("world", "binding-receipt-v1"),
        fields: vec![
            (Symbol::new("binding"), Datum::String(render(binding))),
            (Symbol::new("scope"), Datum::String(scope.to_owned())),
            (Symbol::new("subject"), Datum::String(render(subject))),
        ],
    };
    node.content_id().map_err(|_| Fault::Bound)
}

fn binding_id() -> Result<ContentId, Fault> {
    identity("binding", "world/bind")
}

fn subject_id(byte: u8) -> Result<ContentId, Fault> {
    identity("subject", &format!("{byte:02x}"))
}

fn identity(kind: &str, name: &str) -> Result<ContentId, Fault> {
    let node = Datum::Node {
        tag: Symbol::qualified("world", "bind-identity-v1"),
        fields: vec![
            (Symbol::new("kind"), Datum::String(kind.to_owned())),
            (Symbol::new("name"), Datum::String(name.to_owned())),
        ],
    };
    node.content_id().map_err(|_| Fault::Bound)
}

struct Party {
    kind: &'static str,
    id: ContentId,
}

fn party(kind: &'static str, name: &str) -> Result<Party, Fault> {
    Ok(Party {
        kind,
        id: identity(kind, name)?,
    })
}

fn presentation_reason(expected: &Party, presented: &Party) -> &'static str {
    if presented.kind != expected.kind {
        "domain-confusion"
    } else if presented.id == expected.id {
        "accepted"
    } else {
        "wrong-binding"
    }
}

fn render(id: &ContentId) -> String {
    let digest: String = id.bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}:{digest}", id.algorithm.as_qualified_str())
}
