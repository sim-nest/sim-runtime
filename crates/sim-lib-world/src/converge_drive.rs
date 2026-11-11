// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Published revision calls for one converge case.

use std::collections::{BTreeMap, BTreeSet};

use sim_lib_roadmap_runner::{
    Fence, OpenClaim, ProofOutcome, Proposal, ProposalBody, Revision, RevisionOpen, Step,
    TypedFailure, open_revision, proof_frontier, propose, seal_id,
};

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
            Self::Refused => "converge-refused",
        }
    }
}

pub(crate) fn preserve() -> Result<Law, Fault> {
    let (mut revision, mut invokes) = opened(&[("note", b"one")], &[("a", &["note"])])?;
    let origin = revision.id.clone();
    invokes += 1;
    match propose(
        &mut revision,
        complete("ada", "ada", Fence::Commit, "note", b"two"),
    )
    .map_err(refused)?
    {
        Step::Refused {
            reason,
            revision: id,
        } if reason == "self-approval" && id == origin && revision.id == origin => {}
        _ => return Err(Fault::Bound),
    }
    invokes += 1;
    match propose(
        &mut revision,
        complete("ada", "bea", Fence::BeforeCommit, "note", b"two"),
    )
    .map_err(refused)?
    {
        Step::Held { revision: id }
            if id == origin && revision.is_uncertain() && revision.id == origin => {}
        _ => return Err(Fault::Bound),
    }
    if revision.images().get("note").map(Vec::as_slice) != Some(b"one".as_slice()) {
        return Err(Fault::Bound);
    }
    invokes += 1;
    match propose(
        &mut revision,
        complete("ada", "bea", Fence::Commit, "note", b"two"),
    )
    .map_err(refused)?
    {
        Step::Refused {
            reason,
            revision: id,
        } if reason == "uncertain" && id == origin && revision.id == origin => {}
        _ => return Err(Fault::Bound),
    }
    Ok(Law {
        detail: "self-approval held 1 uncertain 1 same 1 later uncertain".to_owned(),
        invokes,
    })
}

pub(crate) fn invalidate() -> Result<Law, Fault> {
    let (mut revision, mut invokes) = opened(
        &[("note", b"one"), ("other", b"keep")],
        &[("a", &["note"]), ("b", &["other"])],
    )?;
    let origin = revision.id.clone();
    invokes += 1;
    match propose(
        &mut revision,
        complete("ada", "bea", Fence::Commit, "note", b"two"),
    )
    .map_err(refused)?
    {
        Step::Invalidated {
            affected,
            closed,
            revision: id,
            ..
        } if affected == names(&["a"])
            && closed == names(&["b"])
            && id != origin
            && revision.id == id
            && revision
                .claims()
                .get("a")
                .is_some_and(|claim| !claim.closed)
            && revision.claims().get("b").is_some_and(|claim| claim.closed) => {}
        _ => return Err(Fault::Bound),
    }
    Ok(Law {
        detail: "affected a closed b changed 1".to_owned(),
        invokes,
    })
}

pub(crate) fn oscillate() -> Result<Law, Fault> {
    let (mut revision, mut invokes) = opened(&[("note", b"one")], &[("a", &["note"])])?;
    let origin = revision.id.clone();
    invokes += 1;
    match propose(
        &mut revision,
        complete("ada", "bea", Fence::Commit, "note", b"one"),
    )
    .map_err(refused)?
    {
        Step::Oscillation {
            delta,
            revision: id,
            ..
        } if delta.is_empty() && id == origin && revision.id == origin => {}
        _ => return Err(Fault::Bound),
    }
    invokes += 1;
    let next = match propose(
        &mut revision,
        complete("ada", "bea", Fence::Commit, "note", b"two"),
    )
    .map_err(refused)?
    {
        Step::Invalidated { revision: id, .. } if id != origin && revision.id == id => id,
        _ => return Err(Fault::Bound),
    };
    invokes += 1;
    match propose(
        &mut revision,
        complete("ada", "bea", Fence::Commit, "note", b"one"),
    )
    .map_err(refused)?
    {
        Step::Oscillation {
            delta,
            revision: id,
            ..
        } if delta == names(&["note"]) && id == next && revision.id == next => {}
        _ => return Err(Fault::Bound),
    }
    Ok(Law {
        detail: "empty 1 seen note same 1".to_owned(),
        invokes,
    })
}

pub(crate) fn parallel() -> Result<Law, Fault> {
    let mut outcomes = BTreeMap::new();
    outcomes.insert("a".to_owned(), ProofOutcome::Failed);
    outcomes.insert("b".to_owned(), ProofOutcome::Failed);
    let mut invokes = 0;
    invokes += 1;
    let forward = proof_frontier(&["a".to_owned(), "b".to_owned()], &outcomes).map_err(refused)?;
    invokes += 1;
    let backward = proof_frontier(&["b".to_owned(), "a".to_owned()], &outcomes).map_err(refused)?;
    let text = class_text(&forward.failures);
    if forward.failures != backward.failures || text != "a:proof-failed,b:proof-failed" {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: format!("same 1 failures {text}"),
        invokes,
    })
}

pub(crate) fn honest() -> Result<Law, Fault> {
    let (mut revision, mut invokes) = opened(&[("note", b"one")], &[("a", &["note"])])?;
    let origin = revision.id.clone();
    invokes += 1;
    match propose(&mut revision, incomplete("need")).map_err(refused)? {
        Step::Incomplete {
            remaining,
            revision: id,
        } if remaining == ["need"] && id == origin && revision.id == origin => {}
        _ => return Err(Fault::Bound),
    }
    invokes += 1;
    match propose(&mut revision, malformed("bad")).map_err(refused)? {
        Step::Malformed {
            reason,
            revision: id,
        } if reason == "bad" && id == origin && revision.id == origin => {}
        _ => return Err(Fault::Bound),
    }
    if revision.images().get("note").map(Vec::as_slice) != Some(b"one".as_slice()) {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "incomplete need malformed bad same 1".to_owned(),
        invokes,
    })
}

fn opened(facts: &[(&str, &[u8])], claims: &[(&str, &[&str])]) -> Result<(Revision, u64), Fault> {
    let seal = b"seal-v1".to_vec();
    let seal_id = seal_id(&seal).map_err(refused)?;
    let images = facts
        .iter()
        .map(|(path, bytes)| ((*path).to_owned(), bytes.to_vec()))
        .collect();
    let claims = claims
        .iter()
        .map(|(id, inputs)| OpenClaim {
            id: (*id).to_owned(),
            inputs: inputs.iter().map(|input| (*input).to_owned()).collect(),
        })
        .collect();
    let revision = open_revision(RevisionOpen {
        seal,
        seal_id,
        images,
        claims,
    })
    .map_err(refused)?;
    Ok((revision, 2))
}

fn complete(author: &str, checker: &str, fence: Fence, path: &str, bytes: &[u8]) -> Proposal {
    let mut facts = BTreeMap::new();
    facts.insert(path.to_owned(), bytes.to_vec());
    Proposal {
        author: author.to_owned(),
        checker: checker.to_owned(),
        fence,
        body: ProposalBody::Complete {
            id: "proposal".to_owned(),
            facts,
            limit: 64,
        },
    }
}

fn incomplete(need: &str) -> Proposal {
    Proposal {
        author: "ada".to_owned(),
        checker: "bea".to_owned(),
        fence: Fence::Commit,
        body: ProposalBody::Incomplete {
            id: "proposal".to_owned(),
            remaining: vec![need.to_owned()],
        },
    }
}

fn malformed(reason: &str) -> Proposal {
    Proposal {
        author: "ada".to_owned(),
        checker: "bea".to_owned(),
        fence: Fence::Commit,
        body: ProposalBody::Malformed {
            id: "proposal".to_owned(),
            reason: reason.to_owned(),
            unresolved: 1,
            retry: None,
        },
    }
}

fn names(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

fn class_text(failures: &BTreeSet<TypedFailure>) -> String {
    failures
        .iter()
        .map(|failure| format!("{}:{}", failure.unit, failure.class))
        .collect::<Vec<_>>()
        .join(",")
}

fn refused<T>(_: T) -> Fault {
    Fault::Refused
}
