// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Sealed proof leaves executed by the roadmap proof catalog.
//!
//! Each assay conclusion becomes one [`ProofLeaf`]. This catalog is the
//! sensitivity assay, not the portable phase denominator. Cold execution builds
//! one catalog for that union and runs every leaf through
//! [`execute_journaled_proof`]. A pure leaf is a source-deck predicate: the
//! actual deck is the evaluation of the supplied program over the projected
//! text, the expected deck is the evaluation of the sealed program over an
//! independent canonical image of the same facts, and a fact with empty text
//! is a missing claim. A command leaf is a catalog command. Its program is
//! read from that variant, and the qualified
//! route launches it. A scripted receipt is refused before a launch and does
//! not join the denominator. Journal records are the appends the runner
//! journal records after the execution is opened.

use std::collections::BTreeSet;
use std::sync::Arc;

use sim_kernel::{ContentId, Symbol};
use sim_lib_roadmap_runner::{
    ExecutionJournal, ExecutionPins, JournaledProofExecution, Limits, MemoryBackend,
    ProcessCancellation, ProofCatalog, ProofDisposition, ProofLeaf, ProofReceiptStore,
    SandboxAttempt, SandboxLauncher, SandboxRefusal, SandboxRequest, TypedProofReceipt,
    execute_journaled_proof,
};

use crate::WorldError;

const PURE: &str = "pure-check semantic text is canonical and non-empty";
const PROOF_ALPHA: &str = "pure-check proof-code-alpha binds the projected bytes";
const CHECKER_ALPHA: &str = "pure-check checker-alpha binds the projected bytes";

/// One sealed leaf prepared for the roadmap catalog.
#[derive(Clone, Debug)]
pub(super) struct LeafInput {
    /// Catalog leaf name. This is the sealed conclusion id.
    pub(super) name: String,
    /// Program placed on the actual deck and into the retained digest.
    pub(super) code: String,
    /// Fact names in dependency order, including a hidden read when one applies.
    pub(super) facts: Vec<String>,
    /// Provider text aligned with [`Self::facts`].
    pub(super) projected: Vec<String>,
    /// Independent canonical text aligned with [`Self::facts`].
    pub(super) canonical: Vec<String>,
}

/// Verdict returned by the roadmap executor for one leaf.
pub(super) struct Executed {
    /// Catalog leaf name.
    pub(super) name: String,
    /// Whether the catalog returned [`ProofDisposition::Passed`].
    pub(super) passed: bool,
    /// Semantic detail retained from the executor receipt.
    pub(super) semantic_detail: String,
    /// Effect ids present on the receipt.
    pub(super) effects: u64,
    /// Launcher identities present on the receipt.
    pub(super) subprocesses: u64,
    /// Records the runner journal appended for this leaf after it was opened.
    pub(super) journal_records: usize,
}

/// Leaves executed as one catalog, plus the command leaves in that catalog.
pub(super) struct UnionRun {
    /// Executor verdicts in input order.
    pub(super) leaves: Vec<Executed>,
    /// `ProofLeaf::Command` leaves in the catalog that was executed.
    pub(super) owner_commands: u64,
}

/// Source of the predicate this catalog leaf executes.
#[cfg(test)]
pub(super) fn proof_implementation() -> &'static str {
    include_str!("prove_leaf.rs")
}

/// Implementation bytes plus the selected program. The caller supplies this text.
#[cfg(test)]
pub(super) fn executed_code(selected: &str) -> String {
    format!("{}\n{selected}", proof_implementation())
}

/// Counts taken from one executor receipt.
pub(super) struct ObservedLaunch {
    /// Receipts whose effect id is present.
    pub(super) effects: u64,
    /// Receipts whose launcher identity is present.
    pub(super) subprocesses: u64,
    /// Records appended after the execution journal was opened.
    pub(super) journal_records: usize,
}

/// Proof-code text implied by sealed dependency names.
pub(super) fn code_for<'a>(facts: impl IntoIterator<Item = &'a str>) -> &'static str {
    let mut proof = false;
    let mut checker = false;
    for fact in facts {
        if fact == "change/proof-code-alpha" {
            proof = true;
        } else if fact == "change/checker-alpha" {
            checker = true;
        }
    }
    if proof {
        PROOF_ALPHA
    } else if checker {
        CHECKER_ALPHA
    } else {
        PURE
    }
}

/// Canonical image of one raw fact, independent of the semantic-text provider.
pub(super) fn independent_canonical(raw: &str) -> String {
    let mut lines = raw
        .split('\n')
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    lines.sort_unstable();
    lines.join("\n")
}

/// Builds one roadmap catalog for `inputs` and executes every leaf.
pub(super) fn execute_union(inputs: &[LeafInput]) -> Result<UnionRun, WorldError> {
    if inputs.is_empty() {
        return Err(WorldError::Qualification(
            "commissioned assay is empty".to_owned(),
        ));
    }
    let leaves = inputs
        .iter()
        .map(predicate_for)
        .collect::<Result<Vec<_>, _>>()?;
    let catalog = ProofCatalog::new(leaves).map_err(qualify)?;
    let launcher = RefuseLaunch;
    let mut owner_commands = 0u64;
    let mut executed = Vec::with_capacity(inputs.len());
    for input in inputs {
        let leaf = catalog.leaf(&input.name).map_err(qualify)?;
        if program_of(leaf).is_some() {
            owner_commands = owner_commands.saturating_add(1);
        }
        let (receipt, observed) = run_catalog_leaf(&catalog, &input.name, &launcher)?;
        executed.push(Executed {
            name: input.name.clone(),
            passed: receipt.disposition == ProofDisposition::Passed,
            semantic_detail: receipt.semantic_detail,
            effects: observed.effects,
            subprocesses: observed.subprocesses,
            journal_records: observed.journal_records,
        });
    }
    Ok(UnionRun {
        leaves: executed,
        owner_commands,
    })
}

/// Program of a catalog leaf. Pure variants name no owner command.
pub(super) fn program_of(leaf: &ProofLeaf) -> Option<String> {
    match leaf {
        ProofLeaf::Command(command) => Some(command.program.clone()),
        ProofLeaf::ArtifactEquality { .. } | ProofLeaf::SourceDeckPredicate { .. } => None,
    }
}

/// Classifies sealed fact names by the catalog variant they build.
///
/// Opening a view uses this and does not seal the subject again. The cold
/// path executes the catalog built from projected bytes.
#[cfg(test)]
pub(super) fn program_for_facts(facts: &[String]) -> Result<Option<String>, WorldError> {
    if facts.is_empty() {
        return Err(WorldError::Qualification(
            "unavailable-proof has no facts".to_owned(),
        ));
    }
    let code = code_for(facts.iter().map(String::as_str));
    let leaf = predicate_from("sealed-leaf", code, facts, facts, facts)?;
    let catalog = ProofCatalog::new([leaf]).map_err(qualify)?;
    Ok(program_of(catalog.leaf("sealed-leaf").map_err(qualify)?))
}

/// Admits every offered command receipt.
///
/// The sealed cold run offers none. A scripted receipt fails the run.
#[cfg(test)]
pub(super) fn admit_offered(offers: &[(&str, bool)]) -> Result<u64, WorldError> {
    let mut admitted = 0u64;
    for (leaf, scripted) in offers {
        admit_command(leaf, *scripted)?;
        admitted = admitted.saturating_add(1);
    }
    Ok(admitted)
}

/// Admits one command receipt.
///
/// A scripted receipt is refused before the catalog is built and before any
/// launch. A pure sealed leaf has no command. A command leaf is executed
/// through the journaled catalog route.
#[cfg(test)]
pub(super) fn admit_command(leaf: &str, scripted: bool) -> Result<(), WorldError> {
    refuse_if_scripted(scripted)?;
    let bound = super::bind()?;
    let Some(facts) = bound.facts.get(leaf) else {
        return Err(WorldError::Qualification(format!(
            "unavailable-proof {leaf}"
        )));
    };
    let names = facts
        .iter()
        .map(|fact| fact.as_str().to_owned())
        .collect::<Vec<_>>();
    let code = code_for(names.iter().map(String::as_str));
    let predicate = predicate_from(leaf, code, &names, &names, &names)?;
    let catalog = ProofCatalog::new([predicate]).map_err(qualify)?;
    dispatch(&catalog, leaf, &RefuseLaunch)?;
    Ok(())
}

fn predicate_for(input: &LeafInput) -> Result<ProofLeaf, WorldError> {
    predicate_from(
        &input.name,
        &input.code,
        &input.facts,
        &input.projected,
        &input.canonical,
    )
}

fn predicate_from(
    name: &str,
    code: &str,
    facts: &[String],
    projected: &[String],
    canonical: &[String],
) -> Result<ProofLeaf, WorldError> {
    if facts.is_empty() || projected.len() != facts.len() || canonical.len() != facts.len() {
        return Err(WorldError::Qualification(format!(
            "unavailable-proof {name}"
        )));
    }
    let required = facts.iter().cloned().collect::<BTreeSet<_>>();
    let present = facts
        .iter()
        .zip(projected)
        .filter(|(_, text)| !text.is_empty())
        .map(|(fact, _)| fact.clone())
        .collect::<BTreeSet<_>>();
    let sealed = code_for(facts.iter().map(String::as_str));
    Ok(ProofLeaf::SourceDeckPredicate {
        name: name.to_owned(),
        actual_deck: evaluate(code, projected),
        expected_deck: evaluate(sealed, canonical),
        required_claims: required,
        present_claims: present,
    })
}

/// Runs the short program. The known programs bind a canonical image. Any
/// other program still runs, and its text is part of the result.
fn evaluate(program: &str, texts: &[String]) -> String {
    let image = texts
        .iter()
        .map(|text| independent_canonical(text))
        .collect::<Vec<_>>()
        .join("\n");
    match program {
        PURE => format!("canonical nonempty\n{image}"),
        PROOF_ALPHA => format!("alpha binds\n{image}"),
        CHECKER_ALPHA => format!("checker binds\n{image}"),
        other => format!("changed program\n{other}\n{image}"),
    }
}

#[cfg(test)]
fn dispatch(
    catalog: &ProofCatalog,
    name: &str,
    launcher: &dyn SandboxLauncher,
) -> Result<ObservedLaunch, WorldError> {
    let leaf = catalog.leaf(name).map_err(qualify)?;
    let Some(program) = program_of(leaf) else {
        return Err(WorldError::Qualification(format!(
            "catalog leaf {name} requires no owner command"
        )));
    };
    let (receipt, observed) = run_catalog_leaf(catalog, name, launcher)?;
    if receipt.disposition == ProofDisposition::Passed {
        Ok(observed)
    } else {
        Err(WorldError::Qualification(format!(
            "catalog command {program} did not pass"
        )))
    }
}

#[cfg(test)]
fn refuse_if_scripted(scripted: bool) -> Result<(), WorldError> {
    if scripted {
        Err(WorldError::Qualification(
            "scripted process receipt cannot enter the denominator".to_owned(),
        ))
    } else {
        Ok(())
    }
}

fn qualify(error: impl ToString) -> WorldError {
    WorldError::Qualification(error.to_string())
}

fn proof_pins() -> ExecutionPins {
    ExecutionPins {
        conduct: "world-proof".to_owned(),
        policy: "sealed".to_owned(),
        source_deck: ContentId::from_bytes(Symbol::qualified("deck", "sha256-v1"), [1; 32]),
        model_pick: "none".to_owned(),
        runner_generation: "roadmap-runner".to_owned(),
    }
}

struct HeldReceipts {
    inner: std::cell::RefCell<std::collections::BTreeMap<String, TypedProofReceipt>>,
}

impl ProofReceiptStore for HeldReceipts {
    fn inspect(&self, effect_id: &str) -> Option<TypedProofReceipt> {
        self.inner.borrow().get(effect_id).cloned()
    }

    fn record(&self, effect_id: &str, receipt: &TypedProofReceipt) {
        self.inner
            .borrow_mut()
            .insert(effect_id.to_owned(), receipt.clone());
    }
}

fn run_catalog_leaf(
    catalog: &ProofCatalog,
    name: &str,
    launcher: &dyn SandboxLauncher,
) -> Result<(TypedProofReceipt, ObservedLaunch), WorldError> {
    let journal = ExecutionJournal::new(
        Arc::new(MemoryBackend::new()),
        format!("world-proof-{name}"),
        Limits::default(),
    );
    let opened = journal.open(proof_pins(), None).map_err(qualify)?;
    let before = opened.records.len();
    let mut state = opened;
    let receipts = HeldReceipts {
        inner: std::cell::RefCell::new(std::collections::BTreeMap::new()),
    };
    let cancellation = ProcessCancellation::default();
    let receipt = execute_journaled_proof(
        &journal,
        &mut state,
        JournaledProofExecution {
            catalog,
            name,
            launcher,
            receipts: &receipts,
            cancellation: &cancellation,
            observed_at: "sealed".to_owned(),
        },
    )
    .map_err(qualify)?;
    let after = journal.rebuild().map_err(qualify)?.records.len();
    let work = crate::query::HostWork::observe(
        receipt.effect_id.as_deref(),
        receipt.launcher_identity.as_deref(),
    );
    let observed = ObservedLaunch {
        effects: work.effects(),
        subprocesses: work.subprocesses(),
        journal_records: after.saturating_sub(before),
    };
    Ok((receipt, observed))
}

struct RefuseLaunch;

impl SandboxLauncher for RefuseLaunch {
    fn id(&self) -> &str {
        "world-proof-refuse"
    }

    fn launch(&self, _: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
        SandboxAttempt::Refused(SandboxRefusal {
            launcher: self.id().into(),
            reason: "sealed pure leaf has no owner command".into(),
            report: None,
        })
    }
}

/// Qualified-route probe. The command leaf is not a member of the sealed union.
#[cfg(test)]
pub(crate) struct CommandRoute {
    /// Scripted refusal. The recording launcher was not called.
    pub(crate) scripted: String,
    /// Launcher calls across the scripted refusal.
    pub(crate) scripted_calls: usize,
    /// Whether the non-scripted command leaf passed.
    pub(crate) passed: bool,
    /// Launcher calls after the qualified execution.
    pub(crate) executed_calls: usize,
    /// Effect ids observed on the command receipt.
    pub(crate) effects: u64,
    /// Launcher identities observed on the command receipt.
    pub(crate) subprocesses: u64,
    /// Records appended after the command execution was opened.
    pub(crate) journal_records: usize,
}

/// Refuses a scripted command, then executes one catalog command for real.
#[cfg(test)]
pub(super) fn run_catalog_command() -> Result<CommandRoute, WorldError> {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use sim_lib_roadmap_runner::{
        CommandProof, ProofOutputId, SandboxEvidence, SandboxLimits, SandboxReport, SandboxResult,
        StructuredExpectation,
    };

    const NAME: &str = "route/qualified";
    const OUTPUT: &[u8] = b"ok";

    struct Recording {
        calls: AtomicUsize,
    }

    impl SandboxLauncher for Recording {
        fn id(&self) -> &str {
            "world-proof-route"
        }

        fn launch(&self, request: &SandboxRequest, _: &ProcessCancellation) -> SandboxAttempt {
            self.calls.fetch_add(1, Ordering::SeqCst);
            SandboxAttempt::Completed(SandboxResult {
                stdout: OUTPUT.to_vec(),
                stderr: vec![],
                exit_code: 0,
                report: SandboxReport {
                    launcher: self.id().into(),
                    controls: request
                        .policy
                        .requirements()
                        .keys()
                        .map(|control| SandboxEvidence {
                            control: *control,
                            achieved: true,
                            detail: "qualified route".into(),
                        })
                        .collect(),
                    limit_hits: vec![],
                    cleanup: "complete".into(),
                },
            })
        }
    }

    let proof = CommandProof {
        name: NAME.into(),
        effect_id: "effect-route-qualified".into(),
        program: "tool:harmless".into(),
        argv: vec!["--literal".into()],
        working_directory: "/source".into(),
        environment: std::collections::BTreeMap::new(),
        allowed_environment_keys: std::collections::BTreeSet::new(),
        source_mount: "checkout:exact-head".into(),
        scratch_mount: Some("scratch:proof".into()),
        source_read_only: true,
        limits: SandboxLimits {
            cpu_seconds: 1,
            memory_bytes: 32 * 1024 * 1024,
            wall_time_ms: 100,
            process_count: 2,
            file_count: 2,
            file_bytes: 1024,
            output_bytes: 1024,
            stdin_bytes: 1,
        },
        expected: StructuredExpectation {
            stdout: ProofOutputId::sha256(OUTPUT),
            exit_code: 0,
        },
    };
    let catalog = ProofCatalog::new([ProofLeaf::command(proof)]).map_err(qualify)?;
    let launcher = Recording {
        calls: AtomicUsize::new(0),
    };
    let scripted = match refuse_if_scripted(true) {
        Err(error) => error.to_string(),
        Ok(()) => {
            return Err(WorldError::Qualification(
                "scripted receipt was admitted".to_owned(),
            ));
        }
    };
    let scripted_calls = launcher.calls.load(Ordering::SeqCst);
    let observed = dispatch(&catalog, NAME, &launcher)?;
    Ok(CommandRoute {
        scripted,
        scripted_calls,
        passed: true,
        executed_calls: launcher.calls.load(Ordering::SeqCst),
        effects: observed.effects,
        subprocesses: observed.subprocesses,
        journal_records: observed.journal_records,
    })
}
