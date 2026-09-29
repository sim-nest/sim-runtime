// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Prepared execution correlation and independently retained observation custody.

use crate::{CommandRoute, CommandSpec, SandboxAttempt, SandboxRefusal, SandboxRequest};
use sim_kernel::{ContentId, Datum, Error, Result};

/// The owner's pre-cleanup image of one declared output path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetainedOutputImage {
    /// Nothing existed at the path.
    Absent,
    /// A regular file with exactly this content existed at the path.
    File(sim_kernel::ContentId),
    /// Something existed at the path but was not read as a bounded regular
    /// file (a directory, special object, oversized or changing file). It is
    /// evidence of existence only, never of content.
    Present,
    /// The path could not be observed at all (a storage error, a link, which
    /// is never followed, or a parent that does not resolve as a plain
    /// directory, such as a symlinked one).
    /// It is evidence of nothing: postconditions and verifiers refuse it,
    /// while cleanup still proceeds.
    Unobservable,
}

/// Correlates a sandbox with its existing durable dispatch and installed command.
///
/// These identities are data, not a lease or permission to execute. The provider
/// retains and revalidates actual service and resource authority before release.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxInvocation {
    /// Existing coordinator dispatch identity; the sandbox creates no journal.
    pub dispatch: ContentId,
    /// Exact installed command, including its policy and output contract.
    pub command: CommandSpec,
}

impl CommandSpec {
    /// Projects the exact installed sandbox request without changing its bytes.
    /// Returns an error for a process route or invalid rendered invocation.
    pub fn sandbox_request(&self) -> Result<SandboxRequest> {
        let CommandRoute::Sandbox { policy, .. } = self.route() else {
            return Err(Error::Eval("command does not select a sandbox".into()));
        };
        SandboxRequest::new(
            self.program().clone(),
            self.invocation().argv()?,
            self.environment().clone(),
            self.budget().stdin.clone().unwrap_or_default(),
            policy.clone(),
        )
    }
}

impl SandboxInvocation {
    /// Exact correlation carried by a prepared native handoff, without command bytes.
    /// Both complete content identities include their algorithms. This projection
    /// is data only; it neither authorizes a dispatch nor reconstructs live custody.
    pub fn binding_datum(&self) -> Datum {
        crate::SandboxInvocationBinding::from(self).canonical_datum()
    }

    pub(crate) fn validate(
        &self,
        launcher: &str,
        request: &SandboxRequest,
    ) -> std::result::Result<(), Box<SandboxRefusal>> {
        let matches = matches!(self.command.route(), CommandRoute::Sandbox { launcher: selected, .. } if selected == launcher)
            && self
                .command
                .sandbox_request()
                .is_ok_and(|exact| exact == *request);
        if matches {
            Ok(())
        } else {
            Err(Box::new(SandboxRefusal {
                launcher: launcher.into(),
                reason: "prepared command projection mismatch".into(),
                report: None,
            }))
        }
    }
}

/// Independent observation which retains custody across output verification.
///
/// An implementation holds the service/resource handles and workspace exclusion
/// needed to prevent another invocation from changing observed outputs. It does
/// not derive completion from a performer receipt. Dropping this object releases
/// only observation custody, never starts or repeats payload execution.
pub trait SandboxObservation: Send {
    /// Returns the exact installed invocation retained by this observation.
    /// Generic observers refuse because a reconstructed command is not owner
    /// custody.
    fn invocation(&self) -> std::result::Result<&SandboxInvocation, Box<SandboxRefusal>> {
        Err(Box::new(SandboxRefusal {
            launcher: "sandbox-observation".into(),
            reason: "installed sandbox invocation is unavailable".into(),
            report: None,
        }))
    }
    /// Returns the release intent independently bound to the observed execution.
    /// Absence cannot qualify prepared completion; caller-supplied data is not custody.
    fn release(&self) -> Option<&ContentId> {
        None
    }
    /// Returns independently established exit, enforcement and quiescence facts.
    /// Unknown state is not completion; a missing service is not empty custody.
    fn attempt(&self) -> &SandboxAttempt;
    /// Rechecks exact service/resource identity and continuing observation custody.
    /// Call both before and after reading outputs. Failure invalidates the proof.
    fn revalidate(&self) -> std::result::Result<(), Box<SandboxRefusal>>;
    /// Returns a read-only view of this invocation's retained resource object.
    /// The borrow cannot outlive custody. Adapter path maps are not substitutes;
    /// implementations refuse resources they cannot independently bind.
    fn resource(
        &self,
        _name: &str,
    ) -> std::result::Result<&dyn sim_storage_port::HostDirObservation, Box<SandboxRefusal>> {
        Err(Box::new(SandboxRefusal {
            launcher: "sandbox-observation".into(),
            reason: "custody-bound resource observation is unavailable".into(),
            report: None,
        }))
    }
    /// Returns the canonical complete readable-input closure retained by this
    /// exact prepared execution owner.
    ///
    /// The default refuses because a generic or legacy observer cannot prove
    /// completeness. Implementations must derive this value from the same
    /// immutable installation custody used to launch and revalidate the
    /// observed process; a caller-provided file list or mount label is not a
    /// substitute.
    fn complete_readable_input_closure(&self) -> std::result::Result<&Datum, Box<SandboxRefusal>> {
        Err(Box::new(SandboxRefusal {
            launcher: "sandbox-observation".into(),
            reason: "complete readable-input closure is unavailable".into(),
            report: None,
        }))
    }
    /// Returns the platform owner's terminal record of opens against the exact
    /// immutable resources admitted to this invocation.
    ///
    /// This is deliberately narrower than a general syscall or purity proof.
    /// The execution owner must begin observation before release, keep draining
    /// it while the process domain is live, fail closed on loss or overflow,
    /// and bind the result to this invocation and the retained readable-input
    /// closure. Consumers still reobserve each selected file through
    /// [`Self::resource`]; serialized event data never recreates custody.
    ///
    /// Generic observers refuse because Cargo messages, dep-info files and a
    /// caller-created list cannot authenticate native linker, build-script or
    /// procedural-macro reads.
    fn observed_readable_input_opens(&self) -> std::result::Result<&Datum, Box<SandboxRefusal>> {
        Err(Box::new(SandboxRefusal {
            launcher: "sandbox-observation".into(),
            reason: "platform-authenticated readable-input open record is unavailable".into(),
            report: None,
        }))
    }
    /// Consumes the one report-transfer seat and returns its retained custody.
    ///
    /// The returned object remains joined to the same installed invocation and
    /// report owner, and lends report views only inside its own currentness
    /// callback. It is not decoded ownership and cannot be reconstructed from
    /// target files. Implementations must enter their one-shot seat before
    /// transferring custody and refuse every later call even when transfer
    /// fails or unwinds. An authenticated raw unit remains a raw unit: this
    /// transfer makes no completeness or aggregate claim.
    ///
    /// The default refuses because generic observers and independently written
    /// fixtures do not possess a platform-authenticated compiler channel.
    fn take_compiler_semantic_report_custody(
        &mut self,
    ) -> std::result::Result<Box<dyn SandboxCompilerSemanticReportCustody>, Box<SandboxRefusal>>
    {
        Err(Box::new(SandboxRefusal {
            launcher: "sandbox-observation".into(),
            reason: "platform-authenticated compiler semantic report custody is unavailable".into(),
            report: None,
        }))
    }
    /// Returns the owner's retained image of one declared output of a checkout
    /// command.
    ///
    /// A checkout command's working root is scratch that cleanup empties before
    /// independent observation, so its outputs are read by the owner before
    /// emptying and retained with the installed execution. The default refuses
    /// because generic observers retain no such pre-cleanup image.
    fn retained_output_image(
        &self,
        _resource: &str,
        _relative_path: &str,
    ) -> std::result::Result<RetainedOutputImage, Box<SandboxRefusal>> {
        Err(Box::new(SandboxRefusal {
            launcher: "sandbox-observation".into(),
            reason: "platform-retained output image is unavailable".into(),
            report: None,
        }))
    }
    /// Maximum bytes per content observation under this owner's finite policy.
    /// This is a resource ceiling, not stdout/stderr accounting or a file quota.
    /// Zero permits empty files only; no unbounded default is provided.
    fn output_read_limit(&self) -> usize {
        0
    }
    /// Maximum bytes for one retained artifact read from the quota workspace.
    /// This is distinct from captured stdout/stderr and defaults to refusal.
    fn retained_file_read_limit(&self) -> usize {
        0
    }
}

/// Borrowed view of one immutable, platform-authenticated compiler unit report.
///
/// Implementations are reached only through the exact boot-selected
/// [`SandboxObservation`] and remain owned by it for the whole callback. The
/// numerical process fields and digests are correlation data, not standalone
/// authority. This interface deliberately has no aggregate/completeness flag:
/// joining units to the final artifact, input ledger and reachable graph is a
/// separate build-owner responsibility.
pub trait SandboxCompilerSemanticReport: Send + Sync {
    /// Reads the exact immutable bounded report bytes.
    fn read(&self) -> std::result::Result<Vec<u8>, Box<SandboxRefusal>>;
    /// Digest of the independently selected analyzer executable.
    fn analyzer_sha256(&self) -> [u8; 32];
    /// Analyzer PID in the retained platform process view.
    fn analyzer_pid(&self) -> u32;
    /// Analyzer incarnation start time in kernel clock ticks.
    fn analyzer_start_time_ticks(&self) -> u64;
    /// Exact Cargo parent PID in the retained platform process view.
    fn cargo_pid(&self) -> u32;
    /// Cargo parent incarnation start time in kernel clock ticks.
    fn cargo_start_time_ticks(&self) -> u64;
}

/// Transferred owner custody over authenticated compiler-unit reports.
///
/// This object is obtained once from an exact boot-selected
/// [`SandboxObservation`]. It is neither cloneable nor serializable. It retains
/// the platform report objects and the installed-execution currentness needed
/// to reject substitution or replay while a build owner derives and later
/// revalidates an aggregate. Raw report custody is not aggregate completeness.
pub trait SandboxCompilerSemanticReportCustody: Send {
    /// Owner-selected compiler-semantic selection joined to this report set.
    /// Callers must compare it with the exact selection they intend to qualify;
    /// report bytes alone cannot authorize a reconstructed route graph.
    fn selection_binding(&self) -> &ContentId;
    /// Opaque owner-issued identity of this exact channel, report set and order.
    ///
    /// This identity is correlation data for a qualified build receipt. Only
    /// this retained custody object can establish that it is still current.
    fn binding(&self) -> &ContentId;
    /// Lends the exact reports between complete owner currentness observations.
    ///
    /// Implementations must reject replaced invocation/report custody and must
    /// run the same currentness check before and after `accept`. A failed
    /// postcheck discards the callback result. Owner death, revocation, replay,
    /// or any change to the exact report set or order must refuse.
    fn with_current_reports(
        &self,
        accept: &mut SandboxCompilerSemanticReportConsumer<'_>,
    ) -> std::result::Result<(), Box<SandboxRefusal>>;
}

/// Bounded callback which consumes borrowed compiler-report views in place.
///
/// The callback cannot retain the views beyond the owner-controlled currentness
/// interval. Returning an error does not restore the consumed one-shot seat.
pub type SandboxCompilerSemanticReportConsumer<'a> = dyn FnMut(&[&dyn SandboxCompilerSemanticReport]) -> std::result::Result<(), Box<SandboxRefusal>>
    + 'a;

/// Boot-selected read-only authority independent of prepared payload release.
pub trait SandboxObserver: Send + Sync {
    /// Identity of the observation contract, bound into durable verification.
    fn id(&self) -> &str;
    /// Inspects the exact declared destination when acquisition lacks durable acknowledgement.
    /// This is diagnostic reconciliation only: no allocation, payload, cleanup,
    /// replacement or completion authority is granted. Missing state is reported
    /// explicitly, never interpreted as permission to allocate elsewhere.
    fn observe_reservation(
        &self,
        _invocation: &SandboxInvocation,
        _request: &SandboxRequest,
        _destination: &Datum,
    ) -> std::result::Result<Datum, Box<SandboxRefusal>> {
        Err(Box::new(SandboxRefusal {
            launcher: self.id().into(),
            reason: "independent reservation observation is unavailable".into(),
            report: None,
        }))
    }
    /// Acquires independent observation custody for this exact reservation.
    ///
    /// The binding is untrusted correlation data, not deserialized ownership.
    /// This method cannot release payload, clean scratch, or replay a command.
    /// Refuse unavailable, replaced or unowned resources instead of accepting
    /// matching output paths or a process-local performer result.
    fn observe(
        &self,
        invocation: &SandboxInvocation,
        request: &SandboxRequest,
        binding: &Datum,
        release: Option<&ContentId>,
    ) -> std::result::Result<Box<dyn SandboxObservation>, Box<SandboxRefusal>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct GenericObservation {
        attempt: SandboxAttempt,
    }

    impl SandboxObservation for GenericObservation {
        fn attempt(&self) -> &SandboxAttempt {
            &self.attempt
        }

        fn revalidate(&self) -> std::result::Result<(), Box<SandboxRefusal>> {
            Ok(())
        }
    }

    #[test]
    fn generic_observation_cannot_supply_compiler_semantic_custody() {
        let mut observation = GenericObservation {
            attempt: SandboxAttempt::Refused(SandboxRefusal {
                launcher: "fixture/generic".into(),
                reason: "no execution".into(),
                report: None,
            }),
        };
        let error = match observation.take_compiler_semantic_report_custody() {
            Err(error) => error,
            Ok(_) => panic!("generic observation created compiler-report custody"),
        };
        assert_eq!(
            error.reason,
            "platform-authenticated compiler semantic report custody is unavailable"
        );
    }

    #[test]
    fn generic_observation_cannot_supply_native_input_open_records() {
        let observation = GenericObservation {
            attempt: SandboxAttempt::Refused(SandboxRefusal {
                launcher: "fixture/generic".into(),
                reason: "no execution".into(),
                report: None,
            }),
        };
        let error = observation.observed_readable_input_opens().unwrap_err();
        assert_eq!(
            error.reason,
            "platform-authenticated readable-input open record is unavailable"
        );
    }
}
