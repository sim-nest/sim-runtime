//! Durable admission separated from synchronous performance and control delivery.

use std::{error::Error, fmt, sync::Arc};

use sim_lib_journal::JournalBackend;

use crate::{
    CancellationHandle, CancellationSignal, LeaseWindow, LifecyclePerformer, OperationError,
    OperationGrant, OperationIntent, OperationLifecycle, OperationOutcome, PostconditionObserver,
    VerifiedOperationRecord,
};

/// Last boundary crossed by an admission whose owner is retained with its error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationAcceptanceStage {
    /// A writer generation was acquired, but durable admission was not attempted.
    WriterAcquired,
    /// Admission publication was attempted and its durable disposition is uncertain.
    AdmissionPublicationUncertain,
}

/// Admission refusal that distinguishes owner-free validation from retained custody.
///
/// A retained failure cannot be converted into only its cause through the public
/// API. Its writer and exact admission inputs remain private and non-forgeable.
#[must_use = "a retained admission failure owns the exact writer and must be reconciled"]
pub enum OperationAcceptanceError<B: JournalBackend> {
    /// Validation or writer acquisition failed before this call owned a writer.
    Refused(OperationError),
    /// Failure after writer acquisition; the exact admission owner remains held.
    Retained(RetainedOperationAcceptance<B>),
}

impl<B: JournalBackend> OperationAcceptanceError<B> {
    /// Returns the underlying diagnostic without releasing retained custody.
    pub const fn error(&self) -> &OperationError {
        match self {
            Self::Refused(error) => error,
            Self::Retained(failure) => failure.error(),
        }
    }

    /// Returns retained custody when a writer generation had been acquired.
    pub const fn retained(&self) -> Option<&RetainedOperationAcceptance<B>> {
        match self {
            Self::Refused(_) => None,
            Self::Retained(failure) => Some(failure),
        }
    }

    pub(super) fn into_error(self) -> OperationError {
        match self {
            Self::Refused(error) => error,
            Self::Retained(failure) => failure.error,
        }
    }
}

impl<B: JournalBackend> fmt::Debug for OperationAcceptanceError<B> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused(error) => formatter.debug_tuple("Refused").field(error).finish(),
            Self::Retained(failure) => formatter.debug_tuple("Retained").field(failure).finish(),
        }
    }
}

impl<B: JournalBackend> fmt::Display for OperationAcceptanceError<B> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error().fmt(formatter)
    }
}

impl<B: JournalBackend> Error for OperationAcceptanceError<B> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.error())
    }
}

/// Exact writer and admission inputs retained after a post-acquisition failure.
///
/// This value exposes observation and reconciliation only. It cannot execute,
/// cancel, reveal the writer, or transfer any of the retained authority.
#[must_use = "retained admission custody must survive until its disposition is reconciled"]
pub struct RetainedOperationAcceptance<B: JournalBackend> {
    error: OperationError,
    stage: OperationAcceptanceStage,
    operation: Box<AcceptedOperation<B>>,
}

impl<B: JournalBackend> RetainedOperationAcceptance<B> {
    /// Returns the original failure without releasing the retained writer.
    pub const fn error(&self) -> &OperationError {
        &self.error
    }

    /// Returns the exact boundary crossed before failure.
    pub const fn stage(&self) -> OperationAcceptanceStage {
        self.stage
    }

    /// Returns the stable operation identity retained by this owner.
    pub const fn operation_id(&self) -> &crate::OperationId {
        self.operation.intent.id()
    }

    /// Returns the exact grant identity retained by this owner.
    pub const fn grant_id(&self) -> &crate::OperationGrantId {
        self.operation.grant.id()
    }

    /// Re-reads the journal once and verifies it against the retained admission.
    ///
    /// The result is observation only and carries no writer, retry, cancellation,
    /// or execution authority. An absent record is valid after a before-commit cut.
    pub fn revalidate(&self) -> Result<VerifiedOperationRecord, OperationError> {
        self.operation.revalidate()
    }
}

impl<B: JournalBackend> fmt::Debug for RetainedOperationAcceptance<B> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RetainedOperationAcceptance")
            .field("error", &self.error)
            .field("stage", &self.stage)
            .field("operation", self.operation.intent.id())
            .field("grant", self.operation.grant.id())
            .finish_non_exhaustive()
    }
}

/// Failure while binding performer, observer, and cancellation custody.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationCustodyStage {
    /// A durable cancellation existed, but restoring it to the exact signal failed.
    CancellationRestorationUncertain,
}

/// Complete accepted owner retained when cancellation restoration fails.
///
/// No method exposes the performer, observer, cancellation handle, writer, or
/// execution entrypoint. The platform may retain this opaque value, inspect the
/// cause, and verify the retained contracts against a fresh read.
#[must_use = "an accepted operation failure retains all execution and control custody"]
pub struct RetainedOperationCustody<B, P, O>
where
    B: JournalBackend,
    P: LifecyclePerformer,
    O: PostconditionObserver,
{
    error: OperationError,
    stage: OperationCustodyStage,
    custody: Box<AcceptedOperationCustody<B, P, O>>,
}

impl<B, P, O> RetainedOperationCustody<B, P, O>
where
    B: JournalBackend,
    P: LifecyclePerformer,
    O: PostconditionObserver,
{
    /// Returns the original restoration failure without releasing any owner.
    pub const fn error(&self) -> &OperationError {
        &self.error
    }

    /// Returns the exact custody boundary crossed before failure.
    pub const fn stage(&self) -> OperationCustodyStage {
        self.stage
    }

    /// Returns the stable operation identity retained by this owner.
    pub const fn operation_id(&self) -> &crate::OperationId {
        self.custody.operation.intent.id()
    }

    /// Verifies the durable operation against the exact retained owners.
    ///
    /// This performs no dispatch, cancellation, lease acquisition, or retry.
    pub fn verify_retained_contract(
        &self,
    ) -> Result<crate::ContractVerifiedOperation, OperationError> {
        self.custody.verify_retained_contract()
    }
}

impl<B, P, O> fmt::Debug for RetainedOperationCustody<B, P, O>
where
    B: JournalBackend,
    P: LifecyclePerformer,
    O: PostconditionObserver,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RetainedOperationCustody")
            .field("error", &self.error)
            .field("stage", &self.stage)
            .field("operation", self.custody.operation.intent.id())
            .finish_non_exhaustive()
    }
}

/// Accepted operation plus its exact performer, observer, and cancellation owner.
///
/// This is the owner-critical alternative to assembling those values in a
/// downstream crate. Execution borrows this value mutably so a failed run cannot
/// discard custody as part of a plain error conversion.
pub struct AcceptedOperationCustody<B, P, O>
where
    B: JournalBackend,
    P: LifecyclePerformer,
    O: PostconditionObserver,
{
    operation: AcceptedOperation<B>,
    control: CancellationHandle<B>,
    performer: P,
    observer: O,
}

/// One accepted operation and immutable writer generation, independent of clients.
/// A surviving worker owns this value. Client disconnect does not drop it or
/// cancel work. Control handles share its writer, never its execution mutex.
pub struct AcceptedOperation<B: JournalBackend> {
    lifecycle: OperationLifecycle<B>,
    intent: OperationIntent,
    grant: OperationGrant,
    window: LeaseWindow,
    entered: bool,
}

impl<B: JournalBackend> OperationLifecycle<B> {
    /// Durably accepts an exact intent/grant without performing or reserving work.
    /// Acquires one writer generation; old accepted values and control handles
    /// retain their old fence. The returned value never reacquires during run.
    pub fn accept(
        &self,
        intent: &OperationIntent,
        grant: &OperationGrant,
        window: LeaseWindow,
    ) -> Result<AcceptedOperation<B>, OperationAcceptanceError<B>> {
        self.accept_retaining(intent, grant, window)
    }

    /// Accepts an exact operation while preserving every acquired owner on error.
    ///
    /// Input and writer-acquisition refusals carry no owner. Once a writer is
    /// acquired, all failures return opaque retained custody. In particular, a
    /// lost acknowledgement from the admission append is never collapsed into a
    /// retryable plain error.
    pub fn accept_retaining(
        &self,
        intent: &OperationIntent,
        grant: &OperationGrant,
        window: LeaseWindow,
    ) -> Result<AcceptedOperation<B>, OperationAcceptanceError<B>> {
        intent.verify().map_err(OperationAcceptanceError::Refused)?;
        grant.verify().map_err(OperationAcceptanceError::Refused)?;
        if grant.operation != intent.id {
            return Err(OperationAcceptanceError::Refused(
                OperationError::GrantMismatch,
            ));
        }
        self.validate_clock(&window)
            .map_err(OperationAcceptanceError::Refused)?;
        let lease = self
            .journal
            .acquire_lease()
            .map_err(OperationError::from)
            .map_err(OperationAcceptanceError::Refused)?;
        let operation = AcceptedOperation {
            lifecycle: Self {
                journal: self.journal.clone(),
                lease: Some(lease),
                clock: self.clock.clone(),
            },
            intent: intent.clone(),
            grant: grant.clone(),
            window,
            entered: false,
        };
        let existing = match operation.revalidate() {
            Ok(verified) => verified.into_record(),
            Err(error) => {
                return Err(OperationAcceptanceError::Retained(
                    RetainedOperationAcceptance {
                        error,
                        stage: OperationAcceptanceStage::WriterAcquired,
                        operation: Box::new(operation),
                    },
                ));
            }
        };
        if existing.is_none()
            && let Err(error) = operation.lifecycle.append(
                "lifecycle-intent-persisted",
                vec![intent.canonical_datum(), grant.canonical_datum()],
            )
        {
            return Err(OperationAcceptanceError::Retained(
                RetainedOperationAcceptance {
                    error,
                    stage: OperationAcceptanceStage::AdmissionPublicationUncertain,
                    operation: Box::new(operation),
                },
            ));
        }
        Ok(operation)
    }

    pub(super) fn validate_clock(&self, window: &LeaseWindow) -> Result<(), OperationError> {
        match (&self.clock, window.clock()) {
            (Some(clock), Some(domain)) => {
                let reading = clock.read()?;
                if !crate::operation_wire::same_datum(&reading.domain, domain)
                    || reading.tick < window.acquired_at()
                    || reading.tick >= window.expires_at()
                {
                    return Err(OperationError::InvalidLease);
                }
            }
            (None, None) => {}
            _ => return Err(OperationError::InvalidLease),
        }
        Ok(())
    }
}

impl<B: JournalBackend> AcceptedOperation<B> {
    /// Returns the durably accepted exact intent.
    pub const fn intent(&self) -> &OperationIntent {
        &self.intent
    }

    fn revalidate(&self) -> Result<VerifiedOperationRecord, OperationError> {
        let verified = self.lifecycle.verified_record(self.intent.id())?;
        if let Some(record) = verified.record() {
            if record.intent().id() != self.intent.id() {
                return Err(OperationError::ContradictoryIntent);
            }
            if record.grant().id() != self.grant.id() {
                return Err(OperationError::GrantMismatch);
            }
        }
        Ok(verified)
    }

    fn unverified_cancellation_handle(
        &self,
        signal: Arc<dyn CancellationSignal>,
    ) -> CancellationHandle<B> {
        CancellationHandle {
            journal: self.lifecycle.journal.clone(),
            writer: self
                .lifecycle
                .lease
                .clone()
                .expect("accepted operation retains its writer"),
            operation: self.intent.id().clone(),
            grant: self.grant.id().clone(),
            signal,
        }
    }

    fn restore_cancellation(&self, handle: &CancellationHandle<B>) -> Result<(), OperationError> {
        if let Some(cancellation) = self
            .lifecycle
            .record(self.intent.id())?
            .and_then(|record| record.cancellation().cloned())
        {
            handle.request(cancellation.reason().clone())?;
        }
        Ok(())
    }

    /// Binds an independently usable stop signal to this exact accepted run.
    /// The signal must be private to this run. Verified durable cancellation is
    /// restored under the current writer fence before the handle is returned.
    pub fn cancellation_handle(
        &self,
        signal: Arc<dyn CancellationSignal>,
    ) -> Result<CancellationHandle<B>, OperationError> {
        let handle = self.unverified_cancellation_handle(signal);
        self.restore_cancellation(&handle)?;
        Ok(handle)
    }

    /// Binds all execution and control owners before restoring cancellation.
    ///
    /// A restoration error returns an opaque owner-bearing failure. Neither the
    /// error nor its read-only reconciliation methods can extract execution,
    /// cancellation, journal-writer, or native authority.
    pub fn retain_custody<P, O>(
        self,
        performer: P,
        observer: O,
        signal: Arc<dyn CancellationSignal>,
    ) -> Result<AcceptedOperationCustody<B, P, O>, RetainedOperationCustody<B, P, O>>
    where
        P: LifecyclePerformer,
        O: PostconditionObserver,
    {
        let control = self.unverified_cancellation_handle(signal);
        let custody = AcceptedOperationCustody {
            operation: self,
            control,
            performer,
            observer,
        };
        if let Err(error) = custody.operation.restore_cancellation(&custody.control) {
            return Err(RetainedOperationCustody {
                error,
                stage: OperationCustodyStage::CancellationRestorationUncertain,
                custody: Box::new(custody),
            });
        }
        Ok(custody)
    }

    /// Runs or reconciles on the already acquired writer generation exactly once.
    /// The caller retains cancellation handles outside the execution context.
    pub fn run(
        mut self,
        performer: &mut dyn LifecyclePerformer,
        observer: &mut dyn PostconditionObserver,
    ) -> Result<OperationOutcome, OperationError> {
        self.run_retaining(performer, observer)
    }

    fn run_retaining(
        &mut self,
        performer: &mut dyn LifecyclePerformer,
        observer: &mut dyn PostconditionObserver,
    ) -> Result<OperationOutcome, OperationError> {
        if self.entered {
            return Err(OperationError::AcceptanceConsumed);
        }
        self.entered = true;
        self.lifecycle.run_accepted(
            &self.intent,
            &self.grant,
            self.window.clone(),
            performer,
            observer,
        )
    }
}

impl<B, P, O> AcceptedOperationCustody<B, P, O>
where
    B: JournalBackend,
    P: LifecyclePerformer,
    O: PostconditionObserver,
{
    /// Returns the stable identity of the operation held by this custody.
    #[must_use]
    pub const fn operation_id(&self) -> &crate::OperationId {
        self.operation.intent.id()
    }

    /// Returns a control handle bound to this exact retained operation.
    #[must_use]
    pub fn cancellation_handle(&self) -> CancellationHandle<B> {
        self.control.clone()
    }

    /// Runs or reconciles once while keeping every owner in this value.
    ///
    /// Success and failure both retain performer, observer, cancellation, and
    /// journal custody. A second call is refused without dispatch.
    pub fn run_retaining(&mut self) -> Result<OperationOutcome, OperationError> {
        self.operation
            .run_retaining(&mut self.performer, &mut self.observer)
    }

    /// Verifies the latest durable state against the exact retained owners.
    ///
    /// This is observation only and does not dispatch, cancel, or reacquire.
    pub fn verify_retained_contract(
        &self,
    ) -> Result<crate::ContractVerifiedOperation, OperationError> {
        let verified = self.operation.revalidate()?;
        verified.validate_retained_contract(&self.performer.identity(), &self.observer.identity())
    }
}
