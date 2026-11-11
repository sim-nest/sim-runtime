// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Request construction for one landing case.

use std::collections::BTreeMap;

use sim_artifact_facet::{
    ArtifactFacet, ArtifactId, BaseImage, DisclosureDecisionId, IntendedImage, MergePolicy,
    OwnerId, PortableImage, ProjectionId, RegionOwnership, RegionSelector,
};
use sim_lib_roadmap_runner::{
    BoundDisclosure, DisclosureClass, FacetEdit, ModelStore, ModelTip, RepoRequest,
};

use super::Fault;

pub(super) fn ask(
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

pub(super) fn change(
    actor: &OwnerId,
    path: &str,
    base: &str,
    intended: &str,
) -> Result<FacetEdit, Fault> {
    edit(exact(path, actor)?, base, intended)
}

pub(super) fn line_change(
    actor: &OwnerId,
    path: &str,
    base: &str,
    intended: &str,
) -> Result<FacetEdit, Fault> {
    edit(exact_line(path, actor)?, base, intended)
}

pub(super) fn generated_edit(
    generator: &OwnerId,
    path: &str,
    base: &str,
    intended: &str,
) -> Result<FacetEdit, Fault> {
    edit(generated(path, generator)?, base, intended)
}

pub(super) fn public_disclosure() -> Result<BoundDisclosure, Fault> {
    Ok(BoundDisclosure {
        decision: decision("disclosure/model")?,
        class: DisclosureClass::Public,
        public_sink: true,
    })
}

pub(super) fn seed(name: &str, files: &[(&str, &str)]) -> Result<(ModelStore, ModelTip), Fault> {
    let mut store = ModelStore::new();
    let tip = store
        .insert_repository(name, "main", tree(files)?)
        .map_err(refused)?;
    Ok((store, tip))
}

pub(super) fn file(body: &str) -> Result<PortableImage, Fault> {
    PortableImage::file(body.as_bytes(), 0o644).map_err(refused)
}

pub(super) fn owner(name: &str) -> Result<OwnerId, Fault> {
    OwnerId::from_text(name).map_err(refused)
}

pub(super) fn refused<T>(_: T) -> Fault {
    Fault::Refused
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

fn generated(path: &str, generator: &OwnerId) -> Result<ArtifactFacet, Fault> {
    facet(
        path,
        generator,
        RegionSelector::ExactPath(path.to_owned()),
        MergePolicy::Exact,
        RegionOwnership::Generated {
            generator: generator.clone(),
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

fn tree(files: &[(&str, &str)]) -> Result<BTreeMap<String, PortableImage>, Fault> {
    let mut tree = BTreeMap::new();
    for (path, body) in files {
        tree.insert((*path).to_owned(), file(body)?);
    }
    Ok(tree)
}

fn decision(name: &str) -> Result<DisclosureDecisionId, Fault> {
    DisclosureDecisionId::from_text(name).map_err(refused)
}
