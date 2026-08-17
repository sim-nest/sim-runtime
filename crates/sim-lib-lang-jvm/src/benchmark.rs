//! Counter-only adapter for neutral BENCH_2 workloads.
//!
//! This module deliberately owns no clock, sampling, summary, or report. It
//! exposes exact event counts at JVM boundaries so the neutral benchmark owner
//! can retain and attribute measured samples.

use std::collections::BTreeMap;

/// Representative measurement phase selected by a benchmark specification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JvmBenchmarkPhase {
    /// Decode/verification/preparation from an unprepared method.
    ColdPreparation,
    /// Repeated execution of already prepared instructions.
    WarmExecution,
}

/// Exact event counts emitted by the JVM workload adapter.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JvmBenchmarkCounters {
    preparation: u64,
    dispatch: u64,
    resolution: u64,
    allocation: u64,
    root_scanning: u64,
    safepoint_polling: u64,
    work_accounting: u64,
    verifier_checks: u64,
}

impl JvmBenchmarkCounters {
    /// Records a completed preparation boundary and its exact verifier checks.
    pub fn prepared(&mut self, verifier_checks: u64) {
        self.preparation += 1;
        self.verifier_checks += verifier_checks;
    }

    /// Records one prepared instruction dispatch and its work charge.
    pub fn dispatched(&mut self, charged_work: u64) {
        self.dispatch += 1;
        self.work_accounting += charged_work;
    }

    /// Records one ordinary symbolic-resolution boundary.
    pub fn resolved(&mut self) {
        self.resolution += 1;
    }

    /// Records one guest-visible or adapter-fixture allocation boundary.
    pub fn allocated(&mut self) {
        self.allocation += 1;
    }

    /// Records a complete root scan performed at a safepoint.
    pub fn scanned_roots(&mut self) {
        self.root_scanning += 1;
    }

    /// Records one prepared safepoint poll, whether or not it scans roots.
    pub fn polled_safepoint(&mut self) {
        self.safepoint_polling += 1;
    }

    /// Projects the exact BENCH_2 counter vocabulary in lexical order.
    pub fn as_map(&self) -> BTreeMap<&'static str, u64> {
        BTreeMap::from([
            ("allocation", self.allocation),
            ("dispatch", self.dispatch),
            ("preparation", self.preparation),
            ("resolution", self.resolution),
            ("root-scanning", self.root_scanning),
            ("safepoint-polling", self.safepoint_polling),
            ("verifier-checks", self.verifier_checks),
            ("work-accounting", self.work_accounting),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_exposes_only_explicit_event_counts() {
        let mut counters = JvmBenchmarkCounters::default();
        counters.prepared(3);
        counters.dispatched(1);
        counters.resolved();
        counters.allocated();
        counters.polled_safepoint();
        counters.scanned_roots();
        assert_eq!(counters.as_map().values().copied().sum::<u64>(), 10);
        assert_eq!(counters.as_map().len(), 8);
    }
}
