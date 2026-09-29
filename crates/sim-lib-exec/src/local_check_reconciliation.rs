//! Head-bound durable reconciliation for an admitted local check.

use sim_kernel::{ContentId, Datum, Error, Result, Symbol};
use sim_lib_operation_gate::{
    ContractVerifiedOperation, OperationId, OperationIntent, OperationOutcome, OperationOutcomeId,
    OperationStep, ReplayPolicy,
};

use crate::{
    CommandReplayPolicy, CommandSpec, LocalCheckRequest, LocalCheckStatus,
    command_wire::{id_datum, node, u64_datum},
};

/// Exact head of the verified journal snapshot used for a local-check projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalCheckJournalHead {
    sequence: u64,
    entry: ContentId,
}

impl LocalCheckJournalHead {
    /// Returns the journal sequence included in the same verified read.
    pub const fn sequence(&self) -> u64 {
        self.sequence
    }

    /// Returns the semantic entry identity at that sequence.
    pub const fn entry(&self) -> &ContentId {
        &self.entry
    }

    fn canonical_datum(&self) -> Datum {
        node(
            "journal-head-v1",
            vec![
                ("sequence", u64_datum(self.sequence)),
                ("entry", id_datum(&self.entry)),
            ],
        )
    }
}

/// Durable facts for an accepted local check that has no outcome yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalCheckPending {
    last_step: OperationStep,
    dispatch_seen: bool,
    cancellation_requested: bool,
}

impl LocalCheckPending {
    /// Returns the latest durable boundary, never a completion claim.
    pub const fn last_step(&self) -> OperationStep {
        self.last_step
    }

    /// Reports whether at least one durable dispatch exists.
    pub const fn dispatch_seen(&self) -> bool {
        self.dispatch_seen
    }

    /// Reports durable stop intent, not proof that execution is quiescent.
    pub const fn cancellation_requested(&self) -> bool {
        self.cancellation_requested
    }

    fn canonical_datum(&self) -> Datum {
        node(
            "durable-pending-v1",
            vec![
                ("last-durable-step", self.last_step.canonical_datum()),
                ("dispatch-seen", Datum::Bool(self.dispatch_seen)),
                (
                    "cancellation-requested",
                    Datum::Bool(self.cancellation_requested),
                ),
            ],
        )
    }
}

/// Head-bound durable terminal receipt for one admitted local check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalCheckDurableReceipt {
    status: LocalCheckStatus,
    outcome: OperationOutcomeId,
    evidence: Datum,
}

impl LocalCheckDurableReceipt {
    /// Returns the durable outcome classification.
    ///
    /// This is never [`LocalCheckStatus::Refused`]: refusal is an immediate
    /// admission/API result, not a reconstructed journal state.
    pub const fn status(&self) -> LocalCheckStatus {
        self.status
    }

    /// Returns the semantic identity of the persisted lifecycle outcome.
    pub const fn outcome(&self) -> &OperationOutcomeId {
        &self.outcome
    }

    /// Returns canonical evidence derived from that persisted outcome.
    pub const fn evidence(&self) -> &Datum {
        &self.evidence
    }

    fn canonical_datum(&self) -> Datum {
        node(
            "durable-completed-v1",
            vec![
                (
                    "status",
                    Datum::Symbol(Symbol::qualified(
                        "local-check-status",
                        durable_status_name(self.status),
                    )),
                ),
                ("outcome", id_datum(self.outcome.content_id())),
                ("evidence", self.evidence.clone()),
            ],
        )
    }
}

/// Durable local-check state reconstructed without execution authority.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalCheckDurableState {
    /// No record for the expected operation exists in this verified snapshot.
    Absent,
    /// The operation is durable but has no outcome yet.
    Pending(LocalCheckPending),
    /// A durable lifecycle outcome exists.
    Completed(LocalCheckDurableReceipt),
}

impl LocalCheckDurableState {
    fn canonical_datum(&self) -> Datum {
        match self {
            Self::Absent => node("durable-absent-v1", vec![]),
            Self::Pending(pending) => pending.canonical_datum(),
            Self::Completed(receipt) => receipt.canonical_datum(),
        }
    }
}

/// Portable local-check state bound to one request, operation, and verified head.
///
/// This value is observation only. In particular, [`LocalCheckDurableState::Absent`]
/// and [`LocalCheckDurableState::Pending`] do not authorize execution or retry,
/// and neither is converted to [`LocalCheckStatus::Refused`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalCheckReconciliation {
    request: ContentId,
    operation: OperationId,
    journal_head: Option<LocalCheckJournalHead>,
    state: LocalCheckDurableState,
}

impl LocalCheckReconciliation {
    /// Projects one contract-consistent operation read into portable state.
    ///
    /// The opaque input can be obtained only by validating retained performer and
    /// observer contracts against one exact [`sim_lib_operation_gate::VerifiedOperationRecord`].
    /// A raw historical record cannot be projected directly. The resulting
    /// state is nevertheless neutral and does not establish provider authority,
    /// checker qualification, or revocation currentness.
    ///
    /// ```compile_fail
    /// # use sim_lib_exec::{CommandSpec, LocalCheckReconciliation, LocalCheckRequest};
    /// # use sim_lib_operation_gate::VerifiedOperationRecord;
    /// # fn bypass(request: &LocalCheckRequest, command: &CommandSpec, raw: &VerifiedOperationRecord) {
    /// let _ = LocalCheckReconciliation::from_contract_verified_operation(
    ///     request, command, raw,
    /// );
    /// # }
    /// ```
    pub fn from_contract_verified_operation(
        request: &LocalCheckRequest,
        command: &CommandSpec,
        verified: &ContractVerifiedOperation,
    ) -> Result<Self> {
        let request_datum = request.canonical_datum();
        let expected_intent = local_check_intent(request, command)?;
        let expected_operation = expected_intent.id();
        let journal_head = match (verified.journal_sequence(), verified.journal_head()) {
            (Some(sequence), Some(entry)) => Some(LocalCheckJournalHead {
                sequence,
                entry: entry.clone(),
            }),
            (None, None) => None,
            _ => return Err(Error::Eval("incomplete verified journal head".into())),
        };
        let state = verified.record().map_or_else(
            || Ok(LocalCheckDurableState::Absent),
            |record| {
                if record.intent().id() != expected_operation {
                    return Err(Error::Eval(
                        "local-check record does not match expected operation".into(),
                    ));
                }
                match record.outcome() {
                    None => Ok(LocalCheckDurableState::Pending(LocalCheckPending {
                        last_step: record.last_step(),
                        dispatch_seen: !record.dispatches().is_empty(),
                        cancellation_requested: record.cancellation().is_some(),
                    })),
                    Some(outcome) => {
                        let outcome_id = record.outcome_id().cloned().ok_or_else(|| {
                            Error::Eval("durable outcome is missing its identity".into())
                        })?;
                        let (status, evidence) = project_local_check_outcome(outcome);
                        Ok(LocalCheckDurableState::Completed(
                            LocalCheckDurableReceipt {
                                status,
                                outcome: outcome_id,
                                evidence,
                            },
                        ))
                    }
                }
            },
        )?;
        Ok(Self {
            request: request_datum.content_id()?,
            operation: expected_operation.clone(),
            journal_head,
            state,
        })
    }

    /// Returns the semantic identity of the exact local-check request.
    pub const fn request(&self) -> &ContentId {
        &self.request
    }

    /// Returns the expected operation identity.
    pub const fn operation(&self) -> &OperationId {
        &self.operation
    }

    /// Returns the exact verified journal head, when the journal is non-empty.
    pub const fn journal_head(&self) -> Option<&LocalCheckJournalHead> {
        self.journal_head.as_ref()
    }

    /// Returns the reconstructed durable state.
    pub const fn state(&self) -> &LocalCheckDurableState {
        &self.state
    }

    /// Returns the complete canonical projection.
    pub fn canonical_datum(&self) -> Datum {
        node(
            "local-check-reconciliation-v1",
            vec![
                ("request", id_datum(&self.request)),
                ("operation", id_datum(self.operation.content_id())),
                (
                    "journal-head",
                    self.journal_head
                        .as_ref()
                        .map_or(Datum::Nil, LocalCheckJournalHead::canonical_datum),
                ),
                ("state", self.state.canonical_datum()),
            ],
        )
    }

    /// Returns the semantic identity of this exact head-bound projection.
    pub fn content_id(&self) -> Result<ContentId> {
        self.canonical_datum().content_id()
    }
}

/// Derives one canonical operation intent from an exact request and installed
/// command contract.
///
/// Keeping this derivation in the neutral runtime owner prevents platform
/// adapters and receipt readers from independently pairing a request identity
/// with an unrelated, otherwise authentic operation id.
pub fn local_check_intent(
    request: &LocalCheckRequest,
    command: &CommandSpec,
) -> Result<OperationIntent> {
    if request.command() != command.id() {
        return Err(Error::Eval("local check command is not installed".into()));
    }
    OperationIntent::new(
        "local-check/run",
        request.canonical_datum(),
        command.outputs().canonical_datum(),
        match command.replay() {
            CommandReplayPolicy::Idempotent => ReplayPolicy::Idempotent,
            CommandReplayPolicy::ExactlyOnce => ReplayPolicy::ExactlyOnce,
        },
    )
    .map_err(|error| Error::Eval(error.to_string()))
}

/// Projects an immediate lifecycle outcome through the same canonical encoding
/// used by durable local-check receipts.
///
/// Platform adapters use this when an independently changed observer produces a
/// fresh `Uncertain` or `Diverged` result that must not be overwritten by an
/// older terminal receipt. Keeping this projection here prevents a second
/// platform-owned outcome encoding.
#[must_use]
pub fn project_local_check_outcome(outcome: &OperationOutcome) -> (LocalCheckStatus, Datum) {
    match outcome {
        OperationOutcome::AlreadyTrue { evidence } => (
            LocalCheckStatus::AlreadyTrue,
            node(
                "already-true-v1",
                vec![("evidence", id_datum(evidence.content_id()))],
            ),
        ),
        OperationOutcome::Verified { evidence } => (
            LocalCheckStatus::Verified,
            node(
                "verified-v1",
                vec![("evidence", id_datum(evidence.content_id()))],
            ),
        ),
        OperationOutcome::Diverged { observed, expected } => (
            LocalCheckStatus::Diverged,
            node(
                "diverged-v1",
                vec![
                    ("observed", observed.clone()),
                    ("expected", expected.clone()),
                ],
            ),
        ),
        OperationOutcome::Uncertain { last_durable_step } => (
            LocalCheckStatus::Uncertain,
            node(
                "uncertain-v1",
                vec![("last-durable-step", last_durable_step.canonical_datum())],
            ),
        ),
    }
}

fn durable_status_name(status: LocalCheckStatus) -> &'static str {
    match status {
        LocalCheckStatus::AlreadyTrue => "already-true",
        LocalCheckStatus::Verified => "verified",
        LocalCheckStatus::Diverged => "diverged",
        LocalCheckStatus::Uncertain => "uncertain",
        LocalCheckStatus::Refused => {
            unreachable!("refusal is not constructible as a durable receipt")
        }
    }
}
