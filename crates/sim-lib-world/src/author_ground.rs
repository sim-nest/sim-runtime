// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Authored family bytes, roadmap spec, and source deck for the author command.

use std::collections::BTreeMap;

use sim_lib_roadmap_runner::{AuthorError, Family, parse_member};
use sim_roadmap_core::{
    AcceptanceContract, CapabilityEnvelope, ChangeEnvelope, Charter, CheckpointId, CheckpointSpec,
    EffectEnvelope, ImplementationGuide, Limits, OwnerEnvelope, PhaseBody, PhaseId, PhaseOrigin,
    PhaseSpec, ProofPolicy, ResourceEnvelope, RoadmapId, RoadmapSpec, SchemaId,
};
use sim_source_deck::{
    ByteContentId, DeckInput, DeckLimits, FragmentDecoder, IndexFragment, RepositorySnapshot,
    SourceDeck, SourceFile,
};

pub(super) struct Draft {
    pub name: &'static str,
    pub round: &'static str,
    pub stance: &'static str,
    pub path: &'static str,
    pub fact: bool,
    pub decision: &'static str,
    pub consult: Option<&'static str>,
}

pub(super) fn chosen() -> Vec<u8> {
    bytes(Draft {
        name: "chosen",
        round: "2",
        stance: "selected",
        path: "family/chosen.txt",
        fact: true,
        decision: "select this source",
        consult: Some("1"),
    })
}

pub(super) fn notes() -> Vec<u8> {
    bytes(Draft {
        name: "notes",
        round: "1",
        stance: "consulted",
        path: "family/notes.txt",
        fact: true,
        decision: "select this source",
        consult: None,
    })
}

pub(super) fn bytes(draft: Draft) -> Vec<u8> {
    member(
        draft.name,
        draft.round,
        draft.stance,
        draft.path,
        draft.fact,
        draft.decision,
        draft.consult,
    )
}

pub(super) fn member(
    name: &str,
    round: &str,
    stance: &str,
    path: &str,
    fact: bool,
    decision: &str,
    consult: Option<&str>,
) -> Vec<u8> {
    let mut text = format!(
        "family/v1\nname {name}\nround {round}\nstance {stance}\npath {path}\nargument the argument\nwork the work\ngate the gate\nratchet ratchet-{round}\nowner sim-agent-net\nchecker checker-{round}\nroadmap value-roadmap\n"
    );
    if fact {
        text.push_str("fact semantic-tests\n");
    }
    text.push_str("advice keep the source\n");
    text.push_str(&format!("decision {decision}\n"));
    if let Some(consult) = consult {
        text.push_str(&format!("consult {consult}\n"));
    }
    text.into_bytes()
}

pub(super) fn without(bytes: &[u8], section: &str) -> Vec<u8> {
    let text = String::from_utf8(bytes.to_vec()).unwrap_or_default();
    let mut out = String::new();
    for line in text.lines() {
        if line.split_once(' ').is_some_and(|(key, _)| key == section) {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.into_bytes()
}

pub(super) fn family(parts: &[Vec<u8>]) -> Result<Family, AuthorError> {
    let mut members = Vec::with_capacity(parts.len());
    for part in parts {
        members.push(parse_member(part)?);
    }
    Family::from_members(members)
}

pub(super) fn spec() -> RoadmapSpec {
    let root_id = PhaseId::new("root").expect("phase id");
    let leaf_id = PhaseId::new("values").expect("phase id");
    let leaf = PhaseSpec {
        id: leaf_id.clone(),
        parent: Some(root_id.clone()),
        title: "Define values".into(),
        intent: "State one checkpoint".into(),
        body: PhaseBody::Leaf {
            checkpoints: vec![CheckpointSpec {
                id: CheckpointId::new("semantic-tests").expect("checkpoint id"),
                statement: "Canonical laws pass".into(),
            }],
        },
        dependencies: vec![],
        owners: OwnerEnvelope::default(),
        resources: ResourceEnvelope::default(),
        effects: EffectEnvelope::default(),
        capabilities: CapabilityEnvelope::default(),
        changes: ChangeEnvelope::default(),
        acceptance: AcceptanceContract {
            policy: ProofPolicy::All,
            statements: BTreeMap::new(),
        },
        coverage: vec![],
        outputs: BTreeMap::new(),
        guide: ImplementationGuide::default(),
        origin: PhaseOrigin::Authored,
    };
    let root = PhaseSpec {
        id: root_id.clone(),
        parent: None,
        title: "Value roadmap".into(),
        intent: "State bounded authored intent".into(),
        body: PhaseBody::Composite {
            children: vec![leaf_id.clone()],
        },
        dependencies: vec![],
        owners: OwnerEnvelope::default(),
        resources: ResourceEnvelope::default(),
        effects: EffectEnvelope::default(),
        capabilities: CapabilityEnvelope::default(),
        changes: ChangeEnvelope::default(),
        acceptance: AcceptanceContract {
            policy: ProofPolicy::All,
            statements: BTreeMap::new(),
        },
        coverage: vec![],
        outputs: BTreeMap::new(),
        guide: ImplementationGuide::default(),
        origin: PhaseOrigin::Authored,
    };
    RoadmapSpec {
        schema: SchemaId::new("roadmap-v1").expect("schema id"),
        id: RoadmapId::new("value-roadmap").expect("roadmap id"),
        charter: Charter {
            title: "Values".into(),
            intent: "One roadmap states the checkpoint".into(),
        },
        root: root_id,
        phases: [(root.id.clone(), root), (leaf.id.clone(), leaf)]
            .into_iter()
            .collect(),
        imports: BTreeMap::new(),
        limits: Limits::DEFAULT,
    }
}

struct SilentDecoder;

impl FragmentDecoder for SilentDecoder {
    fn decode(&self, _: &[u8]) -> Result<IndexFragment, sim_source_deck::Failure> {
        Err(sim_source_deck::Failure::Decode("no fragment".into()))
    }
}

pub(super) fn deck(files: &[(&str, &[u8])]) -> SourceDeck {
    let decoder = SilentDecoder;
    let built: Vec<SourceFile> = files
        .iter()
        .map(|(path, bytes)| SourceFile {
            owner: "sim-agent-net".into(),
            path: (*path).into(),
            content_id: ByteContentId::of(bytes).expect("content id"),
            bytes: bytes.to_vec(),
        })
        .collect();
    let total = built.iter().map(|file| file.bytes.len()).sum();
    sim_source_deck::build(DeckInput {
        repositories: vec![RepositorySnapshot {
            owner: "sim-agent-net".into(),
            repository: "sim-agent-net".into(),
            revision: "fixture".into(),
        }],
        fragments: vec![],
        certificates: vec![],
        files: built,
        excerpts: vec![],
        specimens: vec![],
        queries: vec![],
        limitations: vec![],
        limits: DeckLimits::strict(1, 0, files.len(), 0, 0, 0, total),
        decoder: &decoder,
    })
    .expect("source deck")
}
