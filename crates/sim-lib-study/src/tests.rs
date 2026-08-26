use super::*;
use sim_lib_journal::MemoryBackend;
use std::sync::atomic::{AtomicBool, Ordering};

fn id(name: &str) -> ContentId {
    ContentId::from_bytes(Symbol::qualified("test", name), Sha256::digest(name).into())
}
fn fixture(samples: u32, attempts: u32) -> (SealedStudy, RequiredClosure) {
    let closure = RequiredClosure {
        task: id("task"),
        harness: id("harness"),
        request: id("request"),
        grader: id("grader"),
    };
    let selectors = Selectors {
        subjects: vec![SubjectRevision::new(id("subject"))],
        tasks: vec![closure.task.clone()],
        harnesses: vec![closure.harness.clone()],
        requests: vec![closure.request.clone()],
        treatments: vec![id("treatment")],
        samples,
    };
    let coordinates = expand(&selectors, &BTreeSet::new(), 20).unwrap();
    let policy = SealPolicy::from_parsed_invocation(
        id("selection"),
        SourceAssertion {
            revision: id("source"),
            clean: true,
        },
        StudyBounds {
            max_coordinates: 20,
            max_attempts_per_coordinate: attempts,
        },
        &Datum::Node {
            tag: Symbol::qualified("test", "invocation"),
            fields: vec![],
        },
        closure.clone(),
    )
    .unwrap();
    (SealedStudy::new(coordinates, policy).unwrap(), closure)
}

struct Resolver(RequiredClosure);
impl ClosureResolver for Resolver {
    fn resolve(&self, _: &StudyCoordinate) -> Result<RequiredClosure, LifecycleError> {
        Ok(self.0.clone())
    }
}
struct Cancel(AtomicBool);
impl Cancel {
    fn no() -> Self {
        Self(AtomicBool::new(false))
    }
}
impl Cancellation for Cancel {
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

struct Fake {
    outcome: AttemptOutcome,
    retry_once: bool,
    calls: usize,
    stale: bool,
    cancel_during: Option<Arc<AtomicBool>>,
    cancelled: usize,
    closure: RequiredClosure,
}
impl StudyExecutor for Fake {
    fn execute(
        &mut self,
        coordinate: &StudyCoordinate,
        claim: &ContentId,
        _: &dyn Cancellation,
    ) -> AttemptEvidence {
        self.calls += 1;
        if let Some(flag) = &self.cancel_during {
            flag.store(true, Ordering::SeqCst);
        }
        AttemptEvidence {
            coordinate: coordinate.content_id().unwrap(),
            claim: if self.stale {
                id("stale-claim")
            } else {
                claim.clone()
            },
            closure: self.closure.clone(),
            outcome: self.outcome,
            objects: vec![b"typed evidence".to_vec()],
            retryable: self.retry_once && self.calls == 1,
        }
    }
    fn cancel(&mut self, _: &ContentId) {
        self.cancelled += 1;
    }
}

#[test]
fn offline_expansion_uses_missing_indexes_below_n() {
    let (study, _) = fixture(3, 2);
    let terminal = BTreeSet::from([study
        .coordinates
        .iter()
        .find(|coordinate| coordinate.sample_index() == 1)
        .unwrap()
        .content_id()
        .unwrap()]);
    let selectors = Selectors {
        subjects: vec![study.coordinates[0].subject().clone()],
        tasks: vec![study.coordinates[0].task().clone()],
        harnesses: vec![study.coordinates[0].harness().clone()],
        requests: vec![study.coordinates[0].request().clone()],
        treatments: vec![study.coordinates[0].treatment().clone()],
        samples: 3,
    };
    let expanded = expand(&selectors, &terminal, 3).unwrap();
    let mut indexes = expanded
        .iter()
        .map(StudyCoordinate::sample_index)
        .collect::<Vec<_>>();
    indexes.sort_unstable();
    assert_eq!(indexes, vec![0, 2]);
}

#[test]
fn fake_executor_completes_and_projection_is_byte_identical_on_replay() {
    let (study, closure) = fixture(2, 2);
    let mut lifecycle = StudyLifecycle::new(MemoryBackend::new());
    lifecycle.install(&study).unwrap();
    let resolver = Resolver(closure.clone());
    let cancel = Cancel::no();
    let mut executor = Fake {
        outcome: AttemptOutcome::Observed,
        retry_once: false,
        calls: 0,
        stale: false,
        cancel_during: None,
        cancelled: 0,
        closure,
    };
    while lifecycle
        .run_one(&study, &resolver, &mut executor, &cancel)
        .unwrap()
    {}
    let first = lifecycle.projection(&study.id).unwrap();
    let second = lifecycle.projection(&study.id).unwrap();
    assert!(first.is_complete());
    assert_eq!(executor.calls, 2);
    assert_eq!(first.canonical_bytes(), second.canonical_bytes());
}

#[test]
fn retry_is_append_only_and_never_duplicates_terminal_coordinate() {
    let (study, closure) = fixture(1, 3);
    let mut lifecycle = StudyLifecycle::new(MemoryBackend::new());
    lifecycle.install(&study).unwrap();
    let resolver = Resolver(closure.clone());
    let cancel = Cancel::no();
    let mut executor = Fake {
        outcome: AttemptOutcome::Observed,
        retry_once: true,
        calls: 0,
        stale: false,
        cancel_during: None,
        cancelled: 0,
        closure,
    };
    assert!(
        lifecycle
            .run_one(&study, &resolver, &mut executor, &cancel)
            .unwrap()
    );
    assert!(
        lifecycle
            .run_one(&study, &resolver, &mut executor, &cancel)
            .unwrap()
    );
    assert!(
        !lifecycle
            .run_one(&study, &resolver, &mut executor, &cancel)
            .unwrap()
    );
    let item = lifecycle
        .projection(&study.id)
        .unwrap()
        .coordinates
        .into_values()
        .next()
        .unwrap();
    assert_eq!(item.attempts.len(), 2);
    assert_eq!(item.state, CoordinateState::Observed);
    assert_eq!(executor.calls, 2);
}

#[test]
fn stale_reply_is_discarded_and_retried_without_scoring_missing_work() {
    let (study, closure) = fixture(1, 3);
    let mut lifecycle = StudyLifecycle::new(MemoryBackend::new());
    lifecycle.install(&study).unwrap();
    let resolver = Resolver(closure.clone());
    let cancel = Cancel::no();
    let mut executor = Fake {
        outcome: AttemptOutcome::Observed,
        retry_once: false,
        calls: 0,
        stale: true,
        cancel_during: None,
        cancelled: 0,
        closure: closure.clone(),
    };
    lifecycle
        .run_one(&study, &resolver, &mut executor, &cancel)
        .unwrap();
    executor.stale = false;
    lifecycle
        .run_one(&study, &resolver, &mut executor, &cancel)
        .unwrap();
    let item = lifecycle
        .projection(&study.id)
        .unwrap()
        .coordinates
        .into_values()
        .next()
        .unwrap();
    assert!(item.attempts[0].discarded);
    assert!(!item.attempts[1].discarded);
    assert_eq!(item.state, CoordinateState::Observed);
}

#[test]
fn cancellation_interrupts_releases_fence_and_explicit_resume_takes_over() {
    let (study, closure) = fixture(1, 3);
    let mut lifecycle = StudyLifecycle::new(MemoryBackend::new());
    lifecycle.install(&study).unwrap();
    let flag = Arc::new(AtomicBool::new(false));
    struct SharedCancel(Arc<AtomicBool>);
    impl Cancellation for SharedCancel {
        fn is_cancelled(&self) -> bool {
            self.0.load(Ordering::SeqCst)
        }
    }
    let cancel = SharedCancel(flag.clone());
    let resolver = Resolver(closure.clone());
    let mut executor = Fake {
        outcome: AttemptOutcome::Observed,
        retry_once: false,
        calls: 0,
        stale: false,
        cancel_during: Some(flag.clone()),
        cancelled: 0,
        closure: closure.clone(),
    };
    lifecycle
        .run_one(&study, &resolver, &mut executor, &cancel)
        .unwrap();
    assert_eq!(executor.cancelled, 1);
    assert_eq!(
        lifecycle
            .projection(&study.id)
            .unwrap()
            .coordinates
            .values()
            .next()
            .unwrap()
            .state,
        CoordinateState::Pending
    );
    flag.store(false, Ordering::SeqCst);
    lifecycle.resume().unwrap();
    executor.cancel_during = None;
    lifecycle
        .run_one(&study, &resolver, &mut executor, &cancel)
        .unwrap();
    let item = lifecycle
        .projection(&study.id)
        .unwrap()
        .coordinates
        .into_values()
        .next()
        .unwrap();
    assert_eq!(item.attempts.len(), 2);
    assert!(item.attempts[0].interrupted);
    assert_eq!(item.state, CoordinateState::Observed);
}

#[test]
fn closure_drift_quarantines_before_executor_and_terminal_admission() {
    let (study, closure) = fixture(1, 2);
    let mut lifecycle = StudyLifecycle::new(MemoryBackend::new());
    lifecycle.install(&study).unwrap();
    let mut drift = closure.clone();
    drift.grader = id("different-grader");
    let mut executor = Fake {
        outcome: AttemptOutcome::Observed,
        retry_once: false,
        calls: 0,
        stale: false,
        cancel_during: None,
        cancelled: 0,
        closure,
    };
    lifecycle
        .run_one(&study, &Resolver(drift), &mut executor, &Cancel::no())
        .unwrap();
    assert_eq!(executor.calls, 0);
    assert_eq!(
        lifecycle
            .projection(&study.id)
            .unwrap()
            .coordinates
            .values()
            .next()
            .unwrap()
            .state,
        CoordinateState::Quarantined
    );
}
// conformance: study tests prove lifecycle execution, retries, replay, and evidence closure.
