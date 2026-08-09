use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Stable identity assigned to an admitted job.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct JobId(u64);

/// Maximum jobs accepted by a queue collection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdmissionLimit(pub usize);

/// Maximum jobs executed by one drain operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkLimit(pub usize);

/// Recorded lifecycle state for a job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobStatus {
    /// Waiting in its typed FIFO.
    Queued,
    /// Executed by an explicit drain.
    Completed,
    /// Cancelled before execution.
    Cancelled,
}

/// Receipt for admission or cancellation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JobReceipt {
    /// Job identity.
    pub id: JobId,
    /// Current status.
    pub status: JobStatus,
}

/// Receipt for a bounded drain.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrainReceipt<K> {
    /// Selected queue class.
    pub class: K,
    /// Jobs completed in FIFO order.
    pub completed: Vec<JobId>,
    /// Whether jobs remain in the class.
    pub pending: bool,
}

/// Receipt for a drain-to-empty checkpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckpointReceipt<K> {
    /// Selected queue class.
    pub class: K,
    /// Jobs completed, including reentrant jobs.
    pub completed: Vec<JobId>,
}

/// Closed failures from bounded admission and checkpoint work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckpointError {
    /// The collection's admission bound was reached.
    AdmissionExhausted,
    /// The checkpoint could not reach empty within its work bound.
    WorkExhausted,
}

type Job<K> = Box<dyn FnOnce(&mut JobQueues<K>)>;
struct Entry<K: Ord> {
    id: JobId,
    run: Job<K>,
}

/// Deterministic, typed FIFO queues driven only by explicit caller operations.
pub struct JobQueues<K: Ord> {
    queues: BTreeMap<K, VecDeque<Entry<K>>>,
    cancelled: BTreeSet<JobId>,
    next_id: u64,
    admitted: usize,
    admission: AdmissionLimit,
}

impl<K: Clone + Ord> JobQueues<K> {
    /// Creates empty queues with a lifetime admission limit.
    pub fn new(admission: AdmissionLimit) -> Self {
        Self {
            queues: BTreeMap::new(),
            cancelled: BTreeSet::new(),
            next_id: 0,
            admitted: 0,
            admission,
        }
    }

    /// Enqueues one job at the tail of its class FIFO.
    pub fn enqueue(
        &mut self,
        class: K,
        job: impl FnOnce(&mut Self) + 'static,
    ) -> Result<JobReceipt, CheckpointError> {
        if self.admitted >= self.admission.0 {
            return Err(CheckpointError::AdmissionExhausted);
        }
        let id = JobId(self.next_id);
        self.next_id = self.next_id.saturating_add(1);
        self.admitted += 1;
        self.queues.entry(class).or_default().push_back(Entry {
            id,
            run: Box::new(job),
        });
        Ok(JobReceipt {
            id,
            status: JobStatus::Queued,
        })
    }

    /// Cancels a queued job. Cancellation is idempotent and receipt-bearing.
    pub fn cancel(&mut self, id: JobId) -> JobReceipt {
        self.cancelled.insert(id);
        JobReceipt {
            id,
            status: JobStatus::Cancelled,
        }
    }

    /// Drains at most `work` jobs from one class.
    pub fn drain(&mut self, class: K, work: WorkLimit) -> DrainReceipt<K> {
        let completed = self.run_selected(&class, work.0);
        let pending = self
            .queues
            .get(&class)
            .is_some_and(|queue| !queue.is_empty());
        DrainReceipt {
            class,
            completed,
            pending,
        }
    }

    /// Drains one class to empty, including jobs enqueued reentrantly into it.
    ///
    /// Fails closed if empty cannot be reached within `work`; unrelated job
    /// classes remain untouched.
    pub fn checkpoint(
        &mut self,
        class: K,
        work: WorkLimit,
    ) -> Result<CheckpointReceipt<K>, CheckpointError> {
        let completed = self.run_selected(&class, work.0);
        if self
            .queues
            .get(&class)
            .is_some_and(|queue| !queue.is_empty())
        {
            return Err(CheckpointError::WorkExhausted);
        }
        Ok(CheckpointReceipt { class, completed })
    }

    fn run_selected(&mut self, class: &K, limit: usize) -> Vec<JobId> {
        let mut completed = Vec::new();
        let mut examined = 0;
        while examined < limit {
            let Some(entry) = self.queues.get_mut(class).and_then(VecDeque::pop_front) else {
                break;
            };
            examined += 1;
            if self.cancelled.remove(&entry.id) {
                continue;
            }
            (entry.run)(self);
            completed.push(entry.id);
        }
        completed
    }
}
