//! Durable study expansion, sealing, execution, and projection lifecycle.

use super::*;

mod wire;

use wire::*;

const FORMAT: u8 = 1;

/// Explicit offline selectors. Empty dimensions are rejected rather than
/// interpreted as ambient discovery.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selectors {
    pub subjects: Vec<SubjectRevision>,
    pub tasks: Vec<ContentId>,
    pub harnesses: Vec<ContentId>,
    pub requests: Vec<ContentId>,
    pub treatments: Vec<ContentId>,
    /// Desired total sample count. Expansion always names missing indexes in
    /// `0..samples`; it never starts above an observed maximum.
    pub samples: u32,
}

/// Caller-supplied bounds over the sealed selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StudyBounds {
    pub max_coordinates: u32,
    pub max_attempts_per_coordinate: u32,
}

/// Purely expands an exact Cartesian product. Existing terminal coordinates
/// are omitted by identity, which gives `N` its total-count semantics.
pub fn expand(
    selectors: &Selectors,
    terminal: &BTreeSet<ContentId>,
    max_coordinates: u32,
) -> Result<Vec<StudyCoordinate>, LifecycleError> {
    if selectors.subjects.is_empty()
        || selectors.tasks.is_empty()
        || selectors.harnesses.is_empty()
        || selectors.requests.is_empty()
        || selectors.treatments.is_empty()
    {
        return Err(LifecycleError::EmptySelector);
    }
    let mut out = Vec::new();
    for subject in &selectors.subjects {
        for task in &selectors.tasks {
            for harness in &selectors.harnesses {
                for request in &selectors.requests {
                    for treatment in &selectors.treatments {
                        for sample in 0..selectors.samples {
                            let coordinate = StudyCoordinate::new(
                                subject.clone(),
                                task.clone(),
                                harness.clone(),
                                request.clone(),
                                treatment.clone(),
                                sample,
                            );
                            if !terminal.contains(&coordinate.content_id()?) {
                                out.push(coordinate);
                                if out.len() > max_coordinates as usize {
                                    return Err(LifecycleError::SelectionBound);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    out.sort_by_key(|coordinate| coordinate.content_id().expect("validated coordinate"));
    out.dedup_by(|a, b| a.content_id().ok() == b.content_id().ok());
    Ok(out)
}

/// A source-tree assertion sealed into study identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceAssertion {
    pub revision: ContentId,
    pub clean: bool,
}

/// Exact closure identities checked both before claim and before admission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequiredClosure {
    pub task: ContentId,
    pub harness: ContentId,
    pub request: ContentId,
    pub grader: ContentId,
}

/// A canonical parsed invocation. No shell string or raw argument vector is
/// accepted by this API.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealPolicy {
    pub selection_policy: ContentId,
    pub source: SourceAssertion,
    pub bounds: StudyBounds,
    /// Content identity of the canonical parsed invocation datum.
    pub invocation: ContentId,
    pub closure: RequiredClosure,
}

impl SealPolicy {
    /// Builds policy from parsed data. Text commands and raw argv are never an
    /// accepted authority form.
    pub fn from_parsed_invocation(
        selection_policy: ContentId,
        source: SourceAssertion,
        bounds: StudyBounds,
        invocation: &Datum,
        closure: RequiredClosure,
    ) -> Result<Self, LifecycleError> {
        Ok(Self {
            selection_policy,
            source,
            bounds,
            invocation: invocation.content_id().map_err(|_| LifecycleError::Datum)?,
            closure,
        })
    }
}

/// Immutable authority for one lifecycle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SealedStudy {
    pub id: ContentId,
    pub coordinates: Vec<StudyCoordinate>,
    pub policy: SealPolicy,
}

impl SealedStudy {
    pub fn new(
        coordinates: Vec<StudyCoordinate>,
        policy: SealPolicy,
    ) -> Result<Self, LifecycleError> {
        if coordinates.is_empty() || coordinates.len() > policy.bounds.max_coordinates as usize {
            return Err(LifecycleError::SelectionBound);
        }
        if policy.bounds.max_attempts_per_coordinate == 0 {
            return Err(LifecycleError::AttemptBound);
        }
        let mut ids = BTreeSet::new();
        for coordinate in &coordinates {
            if !ids.insert(coordinate.content_id()?) {
                return Err(LifecycleError::DuplicateCoordinate);
            }
            if coordinate.task() != &policy.closure.task
                || coordinate.harness() != &policy.closure.harness
                || coordinate.request() != &policy.closure.request
            {
                return Err(LifecycleError::ClosureMismatch);
            }
        }
        let bytes = encode_seal_body(&coordinates, &policy)?;
        let id = object_id(&bytes);
        Ok(Self {
            id,
            coordinates,
            policy,
        })
    }
}

/// Resolves the currently installed closure without performing study effects.
pub trait ClosureResolver {
    fn resolve(&self, coordinate: &StudyCoordinate) -> Result<RequiredClosure, LifecycleError>;
}

/// Cancellation view shared with an executor.
pub trait Cancellation: Send + Sync {
    fn is_cancelled(&self) -> bool;
}

/// Typed result of exactly one executor attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptEvidence {
    pub coordinate: ContentId,
    pub claim: ContentId,
    pub closure: RequiredClosure,
    pub outcome: AttemptOutcome,
    pub objects: Vec<Vec<u8>>,
    pub retryable: bool,
}

/// The sole effectful seam. Implementations own every effect they perform.
pub trait StudyExecutor {
    fn execute(
        &mut self,
        coordinate: &StudyCoordinate,
        claim: &ContentId,
        cancellation: &dyn Cancellation,
    ) -> AttemptEvidence;
    fn cancel(&mut self, claim: &ContentId);
}

/// Exhaustive replay-derived coordinate state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CoordinateState {
    New,
    Pending,
    Claimed,
    RetryWait,
    Observed,
    Unsupported,
    Unresolved,
    Quarantined,
    Complete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptProjection {
    pub revision: u32,
    pub claim: ContentId,
    pub terminal: Option<AttemptOutcome>,
    pub discarded: bool,
    pub interrupted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoordinateProjection {
    pub coordinate: StudyCoordinate,
    pub state: CoordinateState,
    pub attempts: Vec<AttemptProjection>,
}

/// A byte-stable projection reconstructed only from journal objects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StudyProjection {
    pub study: ContentId,
    pub coordinates: BTreeMap<ContentId, CoordinateProjection>,
}

impl StudyProjection {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut out = vec![FORMAT];
        put_id(&mut out, &self.study);
        put_u32(&mut out, self.coordinates.len() as u32);
        for (id, projection) in &self.coordinates {
            put_id(&mut out, id);
            put_u8(&mut out, projection.state as u8);
            put_u32(&mut out, projection.attempts.len() as u32);
            for attempt in &projection.attempts {
                put_u32(&mut out, attempt.revision);
                put_id(&mut out, &attempt.claim);
                put_u8(&mut out, attempt.terminal.map_or(0, outcome_code));
                put_u8(&mut out, u8::from(attempt.discarded));
                put_u8(&mut out, u8::from(attempt.interrupted));
            }
        }
        out
    }
    pub fn is_complete(&self) -> bool {
        self.coordinates.values().all(|c| {
            matches!(
                c.state,
                CoordinateState::Observed
                    | CoordinateState::Unsupported
                    | CoordinateState::Unresolved
                    | CoordinateState::Quarantined
                    | CoordinateState::Complete
            )
        })
    }
}

#[derive(Clone, Debug)]
enum Fact {
    Seal(Box<SealedStudy>),
    Claim {
        study: ContentId,
        coordinate: ContentId,
        revision: u32,
        claim: ContentId,
    },
    Terminal {
        study: ContentId,
        coordinate: ContentId,
        revision: u32,
        claim: ContentId,
        outcome: AttemptOutcome,
        discarded: bool,
    },
    Retry {
        study: ContentId,
        coordinate: ContentId,
        next_revision: u32,
    },
    Interrupted {
        study: ContentId,
        coordinate: ContentId,
        revision: u32,
        claim: ContentId,
    },
}

/// Journal-backed lifecycle. `Arc<B>` lets replay inspect verified immutable
/// objects while `Journal` remains the sole writer.
pub struct StudyLifecycle<B: JournalBackend> {
    backend: Arc<B>,
    journal: Journal<Arc<B>>,
    lease: Option<Lease>,
    accepting_claims: bool,
}

impl<B: JournalBackend> StudyLifecycle<B> {
    pub fn new(backend: B) -> Self {
        let backend = Arc::new(backend);
        Self {
            journal: Journal::new(backend.clone()),
            backend,
            lease: None,
            accepting_claims: true,
        }
    }
    /// Explicit resume is the only operation that obtains a fresh fence after
    /// process loss or takeover.
    pub fn resume(&mut self) -> Result<(), LifecycleError> {
        self.lease = Some(self.journal.acquire_lease()?);
        self.accepting_claims = true;
        Ok(())
    }
    pub fn install(&mut self, study: &SealedStudy) -> Result<(), LifecycleError> {
        if self.lease.is_none() {
            self.resume()?;
        }
        self.append(Fact::Seal(Box::new(study.clone())))
    }
    pub fn projection(&self, study: &ContentId) -> Result<StudyProjection, LifecycleError> {
        let state = self.backend.read_state()?;
        let mut sealed = None;
        let mut facts = Vec::new();
        for entry in state.entries.values() {
            let payload = entry.payloads.first().ok_or(LifecycleError::CorruptFact)?;
            let bytes = state
                .objects
                .get(payload)
                .ok_or(LifecycleError::CorruptFact)?;
            let fact = decode_fact(bytes)?;
            if let Fact::Seal(value) = &fact
                && &value.id == study
            {
                sealed = Some(value.clone());
            }
            facts.push(fact);
        }
        let sealed = sealed.ok_or(LifecycleError::UnknownStudy)?;
        let mut coordinates = BTreeMap::new();
        for coordinate in sealed.coordinates {
            coordinates.insert(
                coordinate.content_id()?,
                CoordinateProjection {
                    coordinate,
                    state: CoordinateState::Pending,
                    attempts: Vec::new(),
                },
            );
        }
        for fact in facts {
            match fact {
                Fact::Claim {
                    study: owner,
                    coordinate,
                    revision,
                    claim,
                } if &owner == study => {
                    let item = coordinates
                        .get_mut(&coordinate)
                        .ok_or(LifecycleError::CorruptFact)?;
                    if item.attempts.iter().any(|a| a.revision == revision) {
                        return Err(LifecycleError::CorruptFact);
                    }
                    item.attempts.push(AttemptProjection {
                        revision,
                        claim,
                        terminal: None,
                        discarded: false,
                        interrupted: false,
                    });
                    item.state = CoordinateState::Claimed;
                }
                Fact::Terminal {
                    study: owner,
                    coordinate,
                    revision,
                    claim,
                    outcome,
                    discarded,
                } if &owner == study => {
                    let item = coordinates
                        .get_mut(&coordinate)
                        .ok_or(LifecycleError::CorruptFact)?;
                    let attempt = item
                        .attempts
                        .iter_mut()
                        .find(|a| a.revision == revision && a.claim == claim)
                        .ok_or(LifecycleError::CorruptFact)?;
                    attempt.terminal = Some(outcome);
                    attempt.discarded = discarded;
                    if !discarded {
                        item.state = state_for_outcome(outcome);
                    }
                }
                Fact::Retry {
                    study: owner,
                    coordinate,
                    next_revision,
                } if &owner == study => {
                    let item = coordinates
                        .get_mut(&coordinate)
                        .ok_or(LifecycleError::CorruptFact)?;
                    if next_revision != item.attempts.len() as u32 {
                        return Err(LifecycleError::CorruptFact);
                    }
                    item.state = CoordinateState::RetryWait;
                }
                Fact::Interrupted {
                    study: owner,
                    coordinate,
                    revision,
                    claim,
                } if &owner == study => {
                    let item = coordinates
                        .get_mut(&coordinate)
                        .ok_or(LifecycleError::CorruptFact)?;
                    let attempt = item
                        .attempts
                        .iter_mut()
                        .find(|a| a.revision == revision && a.claim == claim)
                        .ok_or(LifecycleError::CorruptFact)?;
                    attempt.interrupted = true;
                    item.state = CoordinateState::Pending;
                }
                _ => {}
            }
        }
        Ok(StudyProjection {
            study: study.clone(),
            coordinates,
        })
    }
    pub fn run_one(
        &mut self,
        study: &SealedStudy,
        resolver: &dyn ClosureResolver,
        executor: &mut dyn StudyExecutor,
        cancellation: &dyn Cancellation,
    ) -> Result<bool, LifecycleError> {
        if !self.accepting_claims {
            return Ok(false);
        }
        let projection = self.projection(&study.id)?;
        let Some((coordinate_id, item)) = projection.coordinates.iter().find(|(_, item)| {
            matches!(
                item.state,
                CoordinateState::Pending | CoordinateState::RetryWait
            )
        }) else {
            return Ok(false);
        };
        let coordinate = item.coordinate.clone();
        let revision = item.attempts.len() as u32;
        if revision >= study.policy.bounds.max_attempts_per_coordinate {
            return Err(LifecycleError::AttemptBound);
        }
        if resolver.resolve(&coordinate)? != study.policy.closure {
            return self.quarantine_without_execution(study, coordinate_id, revision);
        }
        if cancellation.is_cancelled() {
            self.accepting_claims = false;
            return Ok(false);
        }
        let claim = claim_id(
            &study.id,
            coordinate_id,
            revision,
            self.lease
                .as_ref()
                .ok_or(LifecycleError::NotResumed)?
                .fence(),
        );
        self.append(Fact::Claim {
            study: study.id.clone(),
            coordinate: coordinate_id.clone(),
            revision,
            claim: claim.clone(),
        })?;
        // The executor runs after the claim transaction has closed.
        let evidence = executor.execute(&coordinate, &claim, cancellation);
        if cancellation.is_cancelled() {
            self.accepting_claims = false;
            executor.cancel(&claim);
            self.append(Fact::Interrupted {
                study: study.id.clone(),
                coordinate: coordinate_id.clone(),
                revision,
                claim,
            })?;
            self.lease = None;
            return Ok(true);
        }
        let closure_live = resolver.resolve(&coordinate)? == study.policy.closure;
        let bound = evidence.coordinate == *coordinate_id
            && evidence.claim == claim
            && evidence.closure == study.policy.closure;
        let discarded = !bound;
        let outcome = if closure_live && bound {
            evidence.outcome
        } else {
            AttemptOutcome::Quarantined
        };
        self.append(Fact::Terminal {
            study: study.id.clone(),
            coordinate: coordinate_id.clone(),
            revision,
            claim,
            outcome,
            discarded,
        })?;
        if discarded || (evidence.retryable && outcome != AttemptOutcome::Quarantined) {
            let next = revision
                .checked_add(1)
                .ok_or(LifecycleError::AttemptBound)?;
            if next < study.policy.bounds.max_attempts_per_coordinate {
                self.append(Fact::Retry {
                    study: study.id.clone(),
                    coordinate: coordinate_id.clone(),
                    next_revision: next,
                })?;
            }
        }
        Ok(true)
    }
    fn quarantine_without_execution(
        &mut self,
        study: &SealedStudy,
        coordinate: &ContentId,
        revision: u32,
    ) -> Result<bool, LifecycleError> {
        let claim = claim_id(
            &study.id,
            coordinate,
            revision,
            self.lease
                .as_ref()
                .ok_or(LifecycleError::NotResumed)?
                .fence(),
        );
        self.append(Fact::Claim {
            study: study.id.clone(),
            coordinate: coordinate.clone(),
            revision,
            claim: claim.clone(),
        })?;
        self.append(Fact::Terminal {
            study: study.id.clone(),
            coordinate: coordinate.clone(),
            revision,
            claim,
            outcome: AttemptOutcome::Quarantined,
            discarded: false,
        })?;
        Ok(true)
    }
    fn append(&mut self, fact: Fact) -> Result<(), LifecycleError> {
        let lease = self.lease.as_ref().ok_or(LifecycleError::NotResumed)?;
        let bytes = encode_fact(&fact)?;
        let object = JournalObject::from_bytes(bytes);
        let expected = self.journal.head()?;
        let sequence = expected.as_ref().map_or(0, |h| h.sequence + 1);
        let entry = JournalEntry::new(
            sequence,
            expected.as_ref().map(|h| h.entry.clone()),
            Symbol::qualified("study", fact_kind(&fact)),
            vec![object.id.clone()],
        );
        self.journal
            .publish(lease, expected.as_ref(), vec![object], vec![entry])?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum LifecycleError {
    #[error("selector dimensions must be explicit and non-empty")]
    EmptySelector,
    #[error("selection exceeds its sealed bound")]
    SelectionBound,
    #[error("attempt bound exhausted")]
    AttemptBound,
    #[error("coordinate appears more than once")]
    DuplicateCoordinate,
    #[error("closure identity does not match the coordinate")]
    ClosureMismatch,
    #[error("lifecycle must be explicitly resumed")]
    NotResumed,
    #[error("study is not installed")]
    UnknownStudy,
    #[error("journal fact is corrupt or noncanonical")]
    CorruptFact,
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Study(#[from] StudyError),
    #[error("datum cannot be canonically sealed")]
    Datum,
}
