// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Published fixed-point calls for one fixed case.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};

use sim_artifact_facet::{
    ArtifactFacet, ArtifactId, DisclosureDecisionId, MergePolicy, OwnerId, ProjectionId,
    RegionOwnership, RegionSelector,
};
use sim_kernel::{ContentId, Datum, Symbol};
use sim_lib_roadmap_runner::{
    FactGenerator, Fence, FixedSeal, FixedStep, OpenClaim, ProofOutcome, Proposal, ProposalBody,
    Revision, RevisionOpen, SOURCE_SEAL, Step, image_id, open_revision, propose, retire_duplicate,
    seal_fixed_point, seal_id,
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
            Self::Refused => "fixed-refused",
        }
    }
}

pub(crate) fn again() -> Result<Law, Fault> {
    let world = world(true)?;
    let accepted = world.revision.clone();
    let mut invokes = world.invokes;
    let first = seal_of(&world, &BTreeSet::new(), None)?;
    invokes += 1;
    let FixedStep::Sealed { seal, work: 2 } = first else {
        return Err(Fault::Bound);
    };
    let second = seal_of(&world, &BTreeSet::new(), Some(&seal))?;
    invokes += 1;
    let FixedStep::ZeroWork { seal: again } = second else {
        return Err(Fault::Bound);
    };
    if again != seal || world.revision != accepted || calls(&world) != 2 {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "sealed 2 zero same 1".to_owned(),
        invokes,
    })
}

pub(crate) fn delete() -> Result<Law, Fault> {
    let world = world(true)?;
    let accepted = world.revision.clone();
    let mut invokes = world.invokes;
    let step = seal_of(&world, &names(&["fact/alpha"]), None)?;
    invokes += 1;
    let FixedStep::Invalidated { closure, retained } = step else {
        return Err(Fault::Bound);
    };
    if world.revision != accepted || calls(&world) != 0 || retained != names(&["fact/beta"]) {
        return Err(Fault::Bound);
    }
    if closure.contains("claim/beta")
        || closure.contains("claim/src")
        || closure.contains("fact/src")
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: format!(
            "closure {} retained {}",
            joined(&closure),
            joined(&retained)
        ),
        invokes,
    })
}

pub(crate) fn gap() -> Result<Law, Fault> {
    let mut world = world(true)?;
    world
        .proofs
        .insert("claim/alpha".to_owned(), ProofOutcome::Failed);
    world
        .proofs
        .insert("claim/beta".to_owned(), ProofOutcome::Failed);
    let accepted = world.revision.clone();
    let mut invokes = world.invokes;
    let step = seal_of(&world, &BTreeSet::new(), None)?;
    invokes += 1;
    let FixedStep::Blocked {
        reason,
        closure,
        repeated: None,
    } = step
    else {
        return Err(Fault::Bound);
    };
    if reason != "proof-failed" || world.revision != accepted || calls(&world) != 0 {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: format!("proof-failed {}", joined(&closure)),
        invokes,
    })
}

pub(crate) fn oscillate() -> Result<Law, Fault> {
    let mut world = world(false)?;
    let original = image_id(world.revision.images()).map_err(refused)?;
    let mut invokes = world.invokes + 1;
    let committed = propose(
        &mut world.revision,
        Proposal {
            author: "author".to_owned(),
            checker: "checker".to_owned(),
            fence: Fence::Commit,
            body: ProposalBody::Complete {
                id: "proposal/1".to_owned(),
                facts: one("fact/alpha", b"alpha-b"),
                limit: 100,
            },
        },
    )
    .map_err(refused)?;
    invokes += 1;
    let Step::Invalidated { affected, .. } = committed else {
        return Err(Fault::Bound);
    };
    if !affected.is_empty() || !world.revision.claims().values().all(|claim| claim.closed) {
        return Err(Fault::Bound);
    }
    let accepted = world.revision.clone();
    let step = seal_of(&world, &BTreeSet::new(), None)?;
    invokes += 1;
    let FixedStep::Blocked {
        reason: "oscillation",
        closure,
        repeated: Some(repeated),
    } = step
    else {
        return Err(Fault::Bound);
    };
    if closure != names(&["fact/alpha"])
        || repeated != original
        || world.revision != accepted
        || world.revision.images().get("fact/alpha").map(Vec::as_slice)
            != Some(b"alpha-b".as_slice())
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "oscillation fact/alpha same 1".to_owned(),
        invokes,
    })
}

pub(crate) fn retire() -> Result<Law, Fault> {
    let world = world(true)?;
    let accepted = world.revision.clone();
    let duplicate = world.alpha.twin();
    duplicate.set("fact/alpha", b"other");
    let mut invokes = world.invokes;
    let step = retire_duplicate(&world.revision, &world.alpha, &duplicate).map_err(refused)?;
    invokes += 1;
    let FixedStep::Retired { work: 1 } = step else {
        return Err(Fault::Bound);
    };
    if duplicate.calls.get() != 0 || world.alpha.calls.get() != 1 || world.revision != accepted {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "retired 1 duplicate 0".to_owned(),
        invokes,
    })
}

struct World {
    revision: Revision,
    facets: Vec<ArtifactFacet>,
    alpha: Gen,
    beta: Gen,
    proofs: BTreeMap<String, ProofOutcome>,
    invokes: u64,
}

fn world(claim_alpha: bool) -> Result<World, Fault> {
    let seal = b"seal-v1".to_vec();
    let mut images = BTreeMap::new();
    images.insert(SOURCE_SEAL.to_owned(), seal.clone());
    images.insert("fact/src".to_owned(), b"src".to_vec());
    images.insert("fact/alpha".to_owned(), b"alpha".to_vec());
    images.insert("fact/extra".to_owned(), b"extra".to_vec());
    images.insert("fact/beta".to_owned(), b"beta".to_vec());
    let mut claims = vec![
        claim("claim/extra", &["fact/extra"]),
        claim("claim/beta", &["fact/beta"]),
        claim("claim/src", &["fact/src"]),
    ];
    if claim_alpha {
        claims.insert(0, claim("claim/alpha", &["fact/alpha"]));
    }
    let revision = open_revision(RevisionOpen {
        seal_id: seal_id(&seal).map_err(refused)?,
        seal,
        images,
        claims,
    })
    .map_err(refused)?;
    let alpha_owner = OwnerId::from_text("owner/alpha").map_err(refused)?;
    let beta_owner = OwnerId::from_text("owner/beta").map_err(refused)?;
    let src_owner = OwnerId::from_text("owner/src").map_err(refused)?;
    let facets = vec![
        facet("artifact/src", &src_owner, "fact/src", false)?,
        facet("artifact/alpha", &alpha_owner, "fact/alpha", true)?,
        facet("artifact/alpha", &alpha_owner, "fact/extra", true)?,
        facet("artifact/beta", &beta_owner, "fact/beta", true)?,
    ];
    let alpha = Gen::new(
        &alpha_owner,
        &["fact/src", SOURCE_SEAL],
        &[("fact/alpha", b"alpha"), ("fact/extra", b"extra")],
    );
    let beta = Gen::new(&beta_owner, &["fact/src"], &[("fact/beta", b"beta")]);
    let mut proofs = BTreeMap::new();
    let mut ids = vec!["claim/extra", "claim/beta", "claim/src"];
    if claim_alpha {
        ids.insert(0, "claim/alpha");
    }
    for id in ids {
        proofs.insert(id.to_owned(), ProofOutcome::Passed);
    }
    Ok(World {
        revision,
        facets,
        alpha,
        beta,
        proofs,
        invokes: 2,
    })
}

fn seal_of(
    world: &World,
    deleted: &BTreeSet<String>,
    prior: Option<&FixedSeal>,
) -> Result<FixedStep, Fault> {
    let generators: [&dyn FactGenerator; 2] = [&world.alpha, &world.beta];
    seal_fixed_point(
        &world.revision,
        &world.facets,
        &generators,
        &world.proofs,
        deleted,
        prior,
    )
    .map_err(refused)
}

fn calls(world: &World) -> u32 {
    world.alpha.calls.get() + world.beta.calls.get()
}

fn facet(
    artifact: &str,
    owner: &OwnerId,
    path: &str,
    generated: bool,
) -> Result<ArtifactFacet, Fault> {
    let ownership = if generated {
        RegionOwnership::Generated {
            generator: owner.clone(),
        }
    } else {
        RegionOwnership::Authored
    };
    ArtifactFacet::new(
        ArtifactId::from_text(artifact).map_err(refused)?,
        owner.clone(),
        RegionSelector::ExactPath(path.to_owned()),
        ProjectionId::from_text("projection/file").map_err(refused)?,
        MergePolicy::Exact,
        DisclosureDecisionId::from_text("disclosure/model").map_err(refused)?,
        ownership,
    )
    .map_err(refused)
}

fn claim(id: &str, inputs: &[&str]) -> OpenClaim {
    OpenClaim {
        id: id.to_owned(),
        inputs: names(inputs),
    }
}

fn one(path: &str, bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let mut facts = BTreeMap::new();
    facts.insert(path.to_owned(), bytes.to_vec());
    facts
}

fn names(ids: &[&str]) -> BTreeSet<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

fn joined(items: &BTreeSet<String>) -> String {
    items.iter().cloned().collect::<Vec<_>>().join(",")
}

fn refused<T>(_: T) -> Fault {
    Fault::Refused
}

struct Gen {
    owner: OwnerId,
    reads: BTreeSet<String>,
    writes: BTreeSet<String>,
    output: RefCell<BTreeMap<String, Vec<u8>>>,
    calls: Cell<u32>,
}

impl Gen {
    fn new(owner: &OwnerId, reads: &[&str], writes: &[(&str, &[u8])]) -> Self {
        let mut output = BTreeMap::new();
        let mut write_set = BTreeSet::new();
        for (path, bytes) in writes {
            write_set.insert((*path).to_owned());
            output.insert((*path).to_owned(), (*bytes).to_vec());
        }
        Self {
            owner: owner.clone(),
            reads: names(reads),
            writes: write_set,
            output: RefCell::new(output),
            calls: Cell::new(0),
        }
    }

    fn twin(&self) -> Self {
        Self {
            owner: self.owner.clone(),
            reads: self.reads.clone(),
            writes: self.writes.clone(),
            output: RefCell::new(self.output.borrow().clone()),
            calls: Cell::new(0),
        }
    }

    fn set(&self, path: &str, bytes: &[u8]) {
        self.output
            .borrow_mut()
            .insert(path.to_owned(), bytes.to_vec());
    }
}

impl FactGenerator for Gen {
    fn owner(&self) -> &OwnerId {
        &self.owner
    }

    fn reads(&self) -> &BTreeSet<String> {
        &self.reads
    }

    fn writes(&self) -> &BTreeSet<String> {
        &self.writes
    }

    fn implementation(&self) -> ContentId {
        let facts = self
            .output
            .borrow()
            .iter()
            .map(|(path, bytes)| Datum::Node {
                tag: Symbol::qualified("roadmap-runner", "QualifiedOutputV1"),
                fields: vec![
                    (Symbol::new("path"), Datum::String(path.clone())),
                    (Symbol::new("bytes"), Datum::Bytes(bytes.clone())),
                ],
            })
            .collect();
        Datum::Node {
            tag: Symbol::qualified("roadmap-runner", "GeneratorImplV1"),
            fields: vec![(Symbol::new("output"), Datum::List(facts))],
        }
        .content_id()
        .expect("qualified implementation")
    }

    fn generate(&self, source: &BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
        self.calls.set(self.calls.get() + 1);
        debug_assert!(self.reads.iter().all(|path| source.contains_key(path)));
        self.output.borrow().clone()
    }
}
