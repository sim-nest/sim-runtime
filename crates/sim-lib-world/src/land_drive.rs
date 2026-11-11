// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Published landing calls for one land case.

use sim_artifact_facet::OwnerId;
use sim_lib_roadmap_runner::{
    ArtifactModel, CleanKind, GitModel, LandCache, LandPlan, LandStatus, LandWhy, ModelStore,
    ModelTip, RepoOutcome, Rollback, land_apply, land_plan, land_recover, land_status, land_why,
    transact_facets,
};

#[path = "land_model.rs"]
mod model;

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
            Self::Refused => "land-refused",
        }
    }
}

pub(crate) fn foreign() -> Result<Law, Fault> {
    let actor = model::owner("owner/src")?;
    let (mut store, base) = model::seed("alpha", &[("src/a.txt", "a\nb\nc\n")])?;
    let mut invokes = 0;
    let mut prep = LandCache::new();
    let tip = apply_exact(
        &mut store,
        &mut prep,
        &base,
        &actor,
        "src/a.txt",
        "a\nb\nc\n",
        "A\nb\nc\n",
    )?;
    invokes += 1;
    if store.image_at("alpha", &base.head, "src/a.txt") != model::file("a\nb\nc\n")? {
        return Err(Fault::Bound);
    }
    let mut cache = LandCache::new();
    let asked = model::ask(
        "alpha",
        &actor,
        &tip,
        &base.head,
        model::public_disclosure()?,
        vec![model::line_change(
            &actor,
            "src/a.txt",
            "a\nb\nc\n",
            "a\nb\nC\n",
        )?],
    );
    let planned = land_plan(&store, &mut cache, &asked).map_err(model::refused)?;
    invokes += 1;
    let LandPlan::WouldCommit {
        files, tip: seen, ..
    } = &planned
    else {
        return Err(Fault::Bound);
    };
    let merged = model::file("A\nb\nC\n")?;
    if seen != &tip
        || files.get("src/a.txt") != Some(&merged)
        || store.tip("alpha").as_ref() != Some(&tip)
        || store.image_at("alpha", &tip.head, "src/a.txt") != model::file("A\nb\nc\n")?
    {
        return Err(Fault::Bound);
    }
    let batch = land_apply(&mut store, &mut cache, &[asked]).map_err(model::refused)?;
    invokes += 1;
    let RepoOutcome::Applied { tip: next, .. } = &batch.outcomes()[0] else {
        return Err(Fault::Bound);
    };
    if store.image_at("alpha", &next.head, "src/a.txt") != merged
        || store.image_at("alpha", &tip.head, "src/a.txt") != model::file("A\nb\nc\n")?
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "would-commit applied kept same 1".to_owned(),
        invokes,
    })
}

pub(crate) fn replay() -> Result<Law, Fault> {
    let actor = model::owner("owner/src")?;
    let (mut store, base) = model::seed("alpha", &[("src/a.txt", "old\n")])?;
    let mut cache = LandCache::new();
    let mut invokes = 0;
    let tip = apply_exact(
        &mut store,
        &mut cache,
        &base,
        &actor,
        "src/a.txt",
        "old\n",
        "new\n",
    )?;
    invokes += 1;
    let again = land_apply(
        &mut store,
        &mut cache,
        &[model::ask(
            "alpha",
            &actor,
            &tip,
            &base.head,
            model::public_disclosure()?,
            vec![model::change(&actor, "src/a.txt", "old\n", "new\n")?],
        )],
    )
    .map_err(model::refused)?;
    invokes += 1;
    let RepoOutcome::Clean {
        kind: CleanKind::AlreadyTrue,
        tip: current,
        ..
    } = &again.outcomes()[0]
    else {
        return Err(Fault::Bound);
    };
    if current != &tip || store.tip("alpha").as_ref() != Some(&tip) {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "already-true same 1".to_owned(),
        invokes,
    })
}

pub(crate) fn recover() -> Result<Law, Fault> {
    let actor = model::owner("owner/src")?;
    let (mut store, base) = model::seed("alpha", &[("src/a.txt", "old\n")])?;
    let mut cache = LandCache::new();
    let mut invokes = 0;
    let batch = land_apply(
        &mut store,
        &mut cache,
        &[model::ask(
            "alpha",
            &actor,
            &base,
            &base.head,
            model::public_disclosure()?,
            vec![model::change(&actor, "src/a.txt", "old\n", "new\n")?],
        )],
    )
    .map_err(model::refused)?;
    invokes += 1;
    let RepoOutcome::Applied {
        tip: next,
        previous,
        ..
    } = &batch.outcomes()[0]
    else {
        return Err(Fault::Bound);
    };
    if previous != &base {
        return Err(Fault::Bound);
    }
    let restored = land_recover(&mut store, &mut cache, "alpha", next, &previous.head)
        .map_err(model::refused)?;
    invokes += 1;
    let Rollback::Restored { tip } = &restored else {
        return Err(Fault::Bound);
    };
    if tip != previous {
        return Err(Fault::Bound);
    }
    let why = land_why(&store, &cache, "alpha");
    invokes += 1;
    if !matches!(
        &why,
        LandWhy::Current { text, head, .. } if text == "restored" && head == &previous.head
    ) || store.image_at("alpha", &previous.head, "src/a.txt") != model::file("old\n")?
        || store.image_at("alpha", &next.head, "src/a.txt") != model::file("new\n")?
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "restored exact kept 1".to_owned(),
        invokes,
    })
}

pub(crate) fn generated() -> Result<Law, Fault> {
    let generator = model::owner("owner/src")?;
    let other = model::owner("owner/other")?;
    let (mut store, tip) = model::seed("alpha", &[("src/a.txt", "old\n")])?;
    let mut cache = LandCache::new();
    let mut invokes = 0;
    let asked = model::ask(
        "alpha",
        &other,
        &tip,
        &tip.head,
        model::public_disclosure()?,
        vec![model::generated_edit(
            &generator,
            "src/a.txt",
            "old\n",
            "new\n",
        )?],
    );
    let planned = land_plan(&store, &mut cache, &asked).map_err(model::refused)?;
    invokes += 1;
    if !matches!(
        &planned,
        LandPlan::Refused { reason, .. } if *reason == "generated-owner-bypass"
    ) || store.tip("alpha").as_ref() != Some(&tip)
    {
        return Err(Fault::Bound);
    }
    let applied = land_apply(&mut store, &mut cache, &[asked]).map_err(model::refused)?;
    invokes += 1;
    if !matches!(
        &applied.outcomes()[0],
        RepoOutcome::Refused { reason, .. } if *reason == "generated-owner-bypass"
    ) || store.image_at("alpha", &tip.head, "src/a.txt") != model::file("old\n")?
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "generated-owner-bypass same 1".to_owned(),
        invokes,
    })
}

pub(crate) fn cache() -> Result<Law, Fault> {
    let actor = model::owner("owner/src")?;
    let (mut store, tip) = model::seed("alpha", &[("src/a.txt", "old\n")])?;
    let mut cache = LandCache::new();
    let mut invokes = 0;
    let unknown = land_why(&store, &cache, "alpha");
    invokes += 1;
    if !matches!(unknown, LandWhy::Unknown { .. }) {
        return Err(Fault::Bound);
    }
    let asked = model::ask(
        "alpha",
        &actor,
        &tip,
        &tip.head,
        model::public_disclosure()?,
        vec![model::change(&actor, "src/a.txt", "old\n", "new\n")?],
    );
    let planned = land_plan(&store, &mut cache, &asked).map_err(model::refused)?;
    invokes += 1;
    if !matches!(planned, LandPlan::WouldCommit { .. }) {
        return Err(Fault::Bound);
    }
    let first = land_why(&store, &cache, "alpha");
    invokes += 1;
    let second = land_why(&store, &cache, "alpha");
    invokes += 1;
    if first != second
        || !matches!(
            &first,
            LandWhy::Current { text, head, .. } if text == "would-commit" && head == &tip.head
        )
    {
        return Err(Fault::Bound);
    }
    transact_facets(&mut store, &[asked]).map_err(model::refused)?;
    invokes += 1;
    let stale = land_why(&store, &cache, "alpha");
    invokes += 1;
    if !matches!(
        &stale,
        LandWhy::Stale { cached_head, current_head, .. }
            if cached_head == &tip.head && current_head != &tip.head
    ) {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "unknown current stale same 1".to_owned(),
        invokes,
    })
}

pub(crate) fn status() -> Result<Law, Fault> {
    let (store, tip) = model::seed("alpha", &[("src/a.txt", "old\n")])?;
    let mut invokes = 0;
    let present = land_status(&store, "alpha");
    invokes += 1;
    let missing = land_status(&store, "missing");
    invokes += 1;
    if !matches!(
        present,
        LandStatus::Present { tip: seen, dirty, .. } if seen == tip && dirty.is_empty()
    ) || !matches!(missing, LandStatus::Missing { .. })
    {
        return Err(Fault::Bound);
    }
    Ok(Law {
        detail: "present missing same 1".to_owned(),
        invokes,
    })
}

fn apply_exact(
    store: &mut ModelStore,
    cache: &mut LandCache,
    tip: &ModelTip,
    actor: &OwnerId,
    path: &str,
    base: &str,
    intended: &str,
) -> Result<ModelTip, Fault> {
    let batch = land_apply(
        store,
        cache,
        &[model::ask(
            "alpha",
            actor,
            tip,
            &tip.head,
            model::public_disclosure()?,
            vec![model::change(actor, path, base, intended)?],
        )],
    )
    .map_err(model::refused)?;
    let RepoOutcome::Applied {
        tip: next,
        previous,
        ..
    } = &batch.outcomes()[0]
    else {
        return Err(Fault::Bound);
    };
    if previous != tip || store.image_at("alpha", &next.head, path) != model::file(intended)? {
        return Err(Fault::Bound);
    }
    Ok(next.clone())
}
