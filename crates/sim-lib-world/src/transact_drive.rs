// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Published model-transaction calls for one transact case.

use std::collections::BTreeMap;

use sim_artifact_facet::{
    ArtifactFacet, ArtifactId, BaseImage, DisclosureDecisionId, FacetError, IntendedImage,
    MergePolicy, OwnerId, PortableImage, ProjectionId, RegionOwnership, RegionSelector,
};
use sim_lib_roadmap_runner::{
    ArtifactModel, BoundDisclosure, CleanKind, DisclosureClass, FacetEdit, GitModel, ModelBatch,
    ModelStore, ModelTip, RepoOutcome, RepoRequest, Rollback, rollback_model, transact_facets,
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
            Self::Refused => "transact-refused",
        }
    }
}

pub(crate) fn apply() -> Result<Law, Fault> {
    let actor = owner("owner/src")?;
    let (mut store, tip) = seed("alpha", &[("src/a.txt", "old\n")])?;
    let mut invokes = 0;
    let applied = exact_commit(&mut store, &tip, &actor, "src/a.txt", "old\n", "new\n")?;
    invokes += 1;
    let again = transact_facets(
        &mut store,
        &[ask(
            "alpha",
            &actor,
            &applied,
            &applied.head,
            public_disclosure()?,
            vec![change(&actor, "src/a.txt", "new\n", "new\n")?],
        )],
    )
    .map_err(refused)?;
    invokes += 1;
    let RepoOutcome::Clean {
        kind: CleanKind::Unchanged,
        tip: current,
        ..
    } = &again.outcomes()[0]
    else {
        return Err(Fault::Bound);
    };
    if current != &applied
        || store.tip("alpha").as_ref() != Some(&applied)
        || store.image_at("alpha", &applied.head, "src/a.txt") != file("new\n")?
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "applied 1 unchanged same 1".to_owned(),
        invokes,
    })
}

pub(crate) fn refuse() -> Result<Law, Fault> {
    let mut invokes = 0;
    let wrong = wrong_head(&mut invokes)?;
    let dirt = dirty(&mut invokes)?;
    let private = private_flow(&mut invokes)?;
    let bypass = generated_bypass(&mut invokes)?;
    let stale = stale_base(&mut invokes)?;
    let escaped = region_escape(&mut invokes)?;
    if [wrong, dirt, private, bypass, stale, escaped]
        != [
            "wrong-head",
            "dirt",
            "private-flow",
            "generated-owner-bypass",
            "stale-base",
            "region-escape",
        ]
        || !path_escape()?
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail:
            "wrong-head dirt private-flow generated-owner-bypass stale-base region-escape same 1"
                .to_owned(),
        invokes,
    })
}

pub(crate) fn overlap() -> Result<Law, Fault> {
    let actor = owner("owner/src")?;
    let (mut store, base) = seed("alpha", &[("src/a.txt", "a\nb\nc\n")])?;
    let mut invokes = 0;
    let tip = exact_commit(
        &mut store,
        &base,
        &actor,
        "src/a.txt",
        "a\nb\nc\n",
        "A\nb\nc\n",
    )?;
    invokes += 1;
    let batch = transact_facets(
        &mut store,
        &[ask(
            "alpha",
            &actor,
            &tip,
            &base.head,
            public_disclosure()?,
            vec![line_change(&actor, "src/a.txt", "a\nb\nc\n", "X\nb\nc\n")?],
        )],
    )
    .map_err(refused)?;
    invokes += 1;
    if refusal(&batch) != "overlap"
        || store.tip("alpha").as_ref() != Some(&tip)
        || store.image_at("alpha", &tip.head, "src/a.txt") != file("A\nb\nc\n")?
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "overlap same 1".to_owned(),
        invokes,
    })
}

pub(crate) fn recover() -> Result<Law, Fault> {
    let actor = owner("owner/src")?;
    let (mut store, alpha) = seed("alpha", &[("src/a.txt", "old\n")])?;
    let beta = store
        .insert_repository("beta", "main", tree(&[("src/b.txt", "a\nb\n")])?)
        .map_err(refused)?;
    let mut invokes = 0;
    let beta_tip = exact_commit(&mut store, &beta, &actor, "src/b.txt", "a\nb\n", "A\nb\n")?;
    invokes += 1;
    let batch = transact_facets(
        &mut store,
        &[
            ask(
                "alpha",
                &actor,
                &alpha,
                &alpha.head,
                public_disclosure()?,
                vec![change(&actor, "src/a.txt", "old\n", "new\n")?],
            ),
            ask(
                "beta",
                &actor,
                &beta_tip,
                &beta.head,
                public_disclosure()?,
                vec![line_change(&actor, "src/b.txt", "a\nb\n", "B\nb\n")?],
            ),
        ],
    )
    .map_err(refused)?;
    invokes += 1;
    if batch.settled()
        || !matches!(batch.outcomes()[0], RepoOutcome::Applied { .. })
        || refusal_at(&batch, 1) != "overlap"
        || store.image_at("alpha", &applied_tip(&batch)?.head, "src/a.txt") != file("new\n")?
        || store.image_at("beta", &beta_tip.head, "src/b.txt") != file("A\nb\n")?
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "recovered overlap same 1".to_owned(),
        invokes,
    })
}

pub(crate) fn rollback() -> Result<Law, Fault> {
    let actor = owner("owner/src")?;
    let (mut store, tip) = seed("alpha", &[("src/a.txt", "old\n")])?;
    let mut invokes = 0;
    let next = exact_commit(&mut store, &tip, &actor, "src/a.txt", "old\n", "new\n")?;
    invokes += 1;
    let restored = rollback_model(&mut store, "alpha", &next, &tip.head).map_err(refused)?;
    invokes += 1;
    let Rollback::Restored { tip: current } = restored else {
        return Err(Fault::Bound);
    };
    if current.head != tip.head
        || store.image_at("alpha", &current.head, "src/a.txt") != file("old\n")?
        || store.image_at("alpha", &next.head, "src/a.txt") != file("new\n")?
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "restored same 1 kept 1".to_owned(),
        invokes,
    })
}

fn wrong_head(invokes: &mut u64) -> Result<&'static str, Fault> {
    let actor = owner("owner/src")?;
    let (mut store, tip) = seed("alpha", &[("src/a.txt", "old\n")])?;
    let mut asked = ask(
        "alpha",
        &actor,
        &tip,
        &tip.head,
        public_disclosure()?,
        vec![change(&actor, "src/a.txt", "old\n", "new\n")?],
    );
    asked.expected_head = "commit/missing".to_owned();
    one_refusal(&mut store, &tip, asked, "src/a.txt", "old\n", invokes)
}

fn dirty(invokes: &mut u64) -> Result<&'static str, Fault> {
    let actor = owner("owner/src")?;
    let (mut store, tip) = seed("alpha", &[("src/a.txt", "old\n")])?;
    if !store.mark_dirt("alpha", "notes.txt") {
        return Err(Fault::Bound);
    }
    let asked = ask(
        "alpha",
        &actor,
        &tip,
        &tip.head,
        public_disclosure()?,
        vec![change(&actor, "src/a.txt", "old\n", "new\n")?],
    );
    let reason = one_refusal(&mut store, &tip, asked, "src/a.txt", "old\n", invokes)?;
    Ok(reason)
}

fn private_flow(invokes: &mut u64) -> Result<&'static str, Fault> {
    let actor = owner("owner/src")?;
    let (mut store, tip) = seed("alpha", &[("src/a.txt", "old\n")])?;
    let disclosure = BoundDisclosure {
        decision: decision("disclosure/model")?,
        class: DisclosureClass::Private,
        public_sink: true,
    };
    let asked = ask(
        "alpha",
        &actor,
        &tip,
        &tip.head,
        disclosure,
        vec![change(&actor, "src/a.txt", "old\n", "new\n")?],
    );
    one_refusal(&mut store, &tip, asked, "src/a.txt", "old\n", invokes)
}

fn generated_bypass(invokes: &mut u64) -> Result<&'static str, Fault> {
    let generator = owner("owner/gen")?;
    let other = owner("owner/other")?;
    let (mut store, tip) = seed("alpha", &[("src/a.txt", "old\n")])?;
    let spec = generated("src/a.txt", &generator)?;
    let asked = ask(
        "alpha",
        &other,
        &tip,
        &tip.head,
        public_disclosure()?,
        vec![edit(spec, "old\n", "new\n")?],
    );
    one_refusal(&mut store, &tip, asked, "src/a.txt", "old\n", invokes)
}

fn stale_base(invokes: &mut u64) -> Result<&'static str, Fault> {
    let actor = owner("owner/src")?;
    let (mut store, tip) = seed("alpha", &[("src/a.txt", "old\n")])?;
    let asked = ask(
        "alpha",
        &actor,
        &tip,
        &tip.head,
        public_disclosure()?,
        vec![change(&actor, "src/a.txt", "stale\n", "new\n")?],
    );
    one_refusal(&mut store, &tip, asked, "src/a.txt", "old\n", invokes)
}

fn region_escape(invokes: &mut u64) -> Result<&'static str, Fault> {
    let actor = owner("owner/src")?;
    let (mut store, tip) = seed(
        "alpha",
        &[("src/a.txt", "old\n"), ("src/b.txt", "a\nb\nc\n")],
    )?;
    let asked = ask(
        "alpha",
        &actor,
        &tip,
        &tip.head,
        public_disclosure()?,
        vec![
            change(&actor, "src/a.txt", "old\n", "new\n")?,
            lines(&actor, "src/b.txt", 2, 3, "a\nb\nc\n", "Z\nb\nc\n")?,
        ],
    );
    let reason = one_refusal(&mut store, &tip, asked, "src/a.txt", "old\n", invokes)?;
    if store.image_at("alpha", &tip.head, "src/b.txt") != file("a\nb\nc\n")? {
        return Err(Fault::Bound);
    }
    Ok(reason)
}

fn path_escape() -> Result<bool, Fault> {
    let actor = owner("owner/src")?;
    let escaped = ArtifactFacet::new(
        ArtifactId::from_text("artifact/escape").map_err(refused)?,
        actor,
        RegionSelector::ExactPath("../outside".to_owned()),
        ProjectionId::from_text("projection/file").map_err(refused)?,
        MergePolicy::Exact,
        decision("disclosure/model")?,
        RegionOwnership::Authored,
    );
    Ok(matches!(escaped, Err(FacetError::InvalidRegion)))
}

fn one_refusal(
    store: &mut ModelStore,
    tip: &ModelTip,
    asked: RepoRequest,
    path: &str,
    body: &str,
    invokes: &mut u64,
) -> Result<&'static str, Fault> {
    let batch = transact_facets(store, &[asked]).map_err(refused)?;
    *invokes += 1;
    if store.tip("alpha").as_ref() != Some(tip)
        || store.image_at("alpha", &tip.head, path) != file(body)?
    {
        return Err(Fault::Bound);
    }
    let reason = refusal(&batch);
    if reason.is_empty() {
        return Err(Fault::Bound);
    }
    Ok(reason)
}

fn exact_commit(
    store: &mut ModelStore,
    tip: &ModelTip,
    actor: &OwnerId,
    path: &str,
    base: &str,
    intended: &str,
) -> Result<ModelTip, Fault> {
    let name = tip_name(store, tip)?;
    let batch = transact_facets(
        store,
        &[ask(
            &name,
            actor,
            tip,
            &tip.head,
            public_disclosure()?,
            vec![change(actor, path, base, intended)?],
        )],
    )
    .map_err(refused)?;
    applied_tip(&batch)
}

fn tip_name(store: &ModelStore, tip: &ModelTip) -> Result<String, Fault> {
    for name in ["alpha", "beta"] {
        if store.tip(name).as_ref() == Some(tip) {
            return Ok(name.to_owned());
        }
    }
    Err(Fault::Bound)
}

fn applied_tip(batch: &ModelBatch) -> Result<ModelTip, Fault> {
    match &batch.outcomes()[0] {
        RepoOutcome::Applied { tip, .. } => Ok(tip.clone()),
        _ => Err(Fault::Bound),
    }
}

fn refusal(batch: &ModelBatch) -> &'static str {
    refusal_at(batch, 0)
}

fn refusal_at(batch: &ModelBatch, index: usize) -> &'static str {
    match batch.outcomes().get(index) {
        Some(RepoOutcome::Refused { reason, .. }) => reason,
        _ => "",
    }
}

fn ask(
    repository: &str,
    actor: &OwnerId,
    tip: &ModelTip,
    base_revision: &str,
    disclosure: BoundDisclosure,
    edits: Vec<FacetEdit>,
) -> RepoRequest {
    RepoRequest {
        repository: repository.to_owned(),
        actor: actor.clone(),
        expected_branch: tip.branch.clone(),
        expected_head: tip.head.clone(),
        base_revision: base_revision.to_owned(),
        disclosure,
        edits,
    }
}

fn change(actor: &OwnerId, path: &str, base: &str, intended: &str) -> Result<FacetEdit, Fault> {
    edit(exact(path, actor)?, base, intended)
}

fn line_change(
    actor: &OwnerId,
    path: &str,
    base: &str,
    intended: &str,
) -> Result<FacetEdit, Fault> {
    edit(exact_line(path, actor)?, base, intended)
}

fn lines(
    actor: &OwnerId,
    path: &str,
    start: u32,
    end: u32,
    base: &str,
    intended: &str,
) -> Result<FacetEdit, Fault> {
    let spec = facet(
        path,
        actor,
        RegionSelector::LineRange {
            path: path.to_owned(),
            start,
            end,
        },
        linewise(),
        RegionOwnership::Authored,
    )?;
    edit(spec, base, intended)
}

fn edit(spec: ArtifactFacet, base: &str, intended: &str) -> Result<FacetEdit, Fault> {
    Ok(FacetEdit {
        base: BaseImage::check(&spec, file(base)?).map_err(refused)?,
        intended: IntendedImage::check(&spec, file(intended)?).map_err(refused)?,
        spec,
    })
}

fn exact(path: &str, actor: &OwnerId) -> Result<ArtifactFacet, Fault> {
    facet(
        path,
        actor,
        RegionSelector::ExactPath(path.to_owned()),
        MergePolicy::Exact,
        RegionOwnership::Authored,
    )
}

fn exact_line(path: &str, actor: &OwnerId) -> Result<ArtifactFacet, Fault> {
    facet(
        path,
        actor,
        RegionSelector::ExactPath(path.to_owned()),
        linewise(),
        RegionOwnership::Authored,
    )
}

fn generated(path: &str, actor: &OwnerId) -> Result<ArtifactFacet, Fault> {
    facet(
        path,
        actor,
        RegionSelector::ExactPath(path.to_owned()),
        MergePolicy::Exact,
        RegionOwnership::Generated {
            generator: actor.clone(),
        },
    )
}

fn facet(
    path: &str,
    actor: &OwnerId,
    region: RegionSelector,
    policy: MergePolicy,
    ownership: RegionOwnership,
) -> Result<ArtifactFacet, Fault> {
    ArtifactFacet::new(
        ArtifactId::from_text(&format!("artifact/{path}")).map_err(refused)?,
        actor.clone(),
        region,
        ProjectionId::from_text("projection/file").map_err(refused)?,
        policy,
        decision("disclosure/model")?,
        ownership,
    )
    .map_err(refused)
}

fn linewise() -> MergePolicy {
    MergePolicy::Linewise {
        max_lines: 32,
        max_cells: 4_096,
    }
}

fn public_disclosure() -> Result<BoundDisclosure, Fault> {
    Ok(BoundDisclosure {
        decision: decision("disclosure/model")?,
        class: DisclosureClass::Public,
        public_sink: true,
    })
}

fn seed(name: &str, files: &[(&str, &str)]) -> Result<(ModelStore, ModelTip), Fault> {
    let mut store = ModelStore::new();
    let tip = store
        .insert_repository(name, "main", tree(files)?)
        .map_err(refused)?;
    Ok((store, tip))
}

fn tree(files: &[(&str, &str)]) -> Result<BTreeMap<String, PortableImage>, Fault> {
    let mut tree = BTreeMap::new();
    for (path, body) in files {
        tree.insert((*path).to_owned(), file(body)?);
    }
    Ok(tree)
}

fn file(body: &str) -> Result<PortableImage, Fault> {
    PortableImage::file(body.as_bytes(), 0o644).map_err(refused)
}

fn owner(name: &str) -> Result<OwnerId, Fault> {
    OwnerId::from_text(name).map_err(refused)
}

fn decision(name: &str) -> Result<DisclosureDecisionId, Fault> {
    DisclosureDecisionId::from_text(name).map_err(refused)
}

fn refused<T>(_: T) -> Fault {
    Fault::Refused
}
