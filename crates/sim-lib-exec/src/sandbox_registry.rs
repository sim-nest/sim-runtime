use crate::{
    ProcessCancellation, SandboxAttempt, SandboxInvocation, SandboxLauncher, SandboxObservation,
    SandboxObserver, SandboxRefusal, SandboxRequest,
};
use sim_kernel::{Datum, Error, Result};
use std::{collections::BTreeMap, sync::Arc};

struct LauncherEntry {
    launcher: Arc<dyn SandboxLauncher>,
    observer: Option<Arc<dyn SandboxObserver>>,
}
/// Boot-built launcher registry; callers select an identity, never a concrete OS type.
#[derive(Default)]
pub struct LauncherRegistry(BTreeMap<String, LauncherEntry>);
impl LauncherRegistry {
    /// Registers one unique boot-selected launcher.
    pub fn register(&mut self, launcher: Arc<dyn SandboxLauncher>) -> Result<()> {
        self.register_entry(launcher, None)
    }
    /// Registers a mandatory prepared path with its separate observation contract.
    /// The selected path cannot fall back to unprepared execution.
    pub fn register_prepared(
        &mut self,
        launcher: Arc<dyn SandboxLauncher>,
        observer: Arc<dyn SandboxObserver>,
    ) -> Result<()> {
        if observer.id().is_empty() || observer.id() == launcher.id() {
            return Err(Error::Eval(
                "prepared sandbox observer is not independent".into(),
            ));
        }
        self.register_entry(launcher, Some(observer))
    }
    fn register_entry(
        &mut self,
        launcher: Arc<dyn SandboxLauncher>,
        observer: Option<Arc<dyn SandboxObserver>>,
    ) -> Result<()> {
        let id = launcher.id();
        if id.is_empty() || self.0.contains_key(id) {
            return Err(Error::Eval("invalid or duplicate sandbox launcher".into()));
        }
        self.0
            .insert(id.into(), LauncherEntry { launcher, observer });
        Ok(())
    }
    /// Dispatches through the selected launcher without caller type dispatch.
    pub fn launch(
        &self,
        id: &str,
        request: &SandboxRequest,
        cancellation: &ProcessCancellation,
    ) -> SandboxAttempt {
        self.0.get(id).map_or_else(
            || {
                SandboxAttempt::Refused(SandboxRefusal {
                    launcher: id.into(),
                    reason: "sandbox launcher is not registered".into(),
                    report: None,
                })
            },
            |v| {
                if v.observer.is_some() {
                    SandboxAttempt::Refused(unavailable(id, "durable preparation is required"))
                } else {
                    v.launcher.launch(request, cancellation)
                }
            },
        )
    }
    /// Returns the selected independent contract, or explicit unprepared mode.
    pub fn observer_identity(&self, id: &str) -> Option<&str> {
        self.0
            .get(id)?
            .observer
            .as_ref()
            .map(|observer| observer.id())
    }
    /// Declares a destination without resource effects; None means unprepared mode.
    pub fn plan_reservation(
        &self,
        id: &str,
        invocation: &SandboxInvocation,
        request: &SandboxRequest,
        cancellation: &ProcessCancellation,
    ) -> std::result::Result<Option<Datum>, Box<SandboxRefusal>> {
        invocation.validate(id, request)?;
        if cancellation.is_cancelled() {
            return Err(Box::new(unavailable(id, "sandbox preparation cancelled")));
        }
        let entry = self
            .0
            .get(id)
            .ok_or_else(|| Box::new(unavailable(id, "launcher is not registered")))?;
        if entry.observer.is_some() {
            entry
                .launcher
                .plan_reservation(invocation, request, cancellation)
                .map(Some)
        } else {
            Ok(None)
        }
    }
    /// Acquires or reconciles a destination after its durable declaration.
    pub fn prepare_reserved(
        &self,
        id: &str,
        invocation: &SandboxInvocation,
        request: &SandboxRequest,
        destination: &Datum,
        cancellation: &ProcessCancellation,
    ) -> std::result::Result<Datum, Box<SandboxRefusal>> {
        invocation.validate(id, request)?;
        if cancellation.is_cancelled() || destination == &Datum::Nil {
            return Err(Box::new(unavailable(
                id,
                "reservation cancelled or destination absent",
            )));
        }
        let entry = self
            .0
            .get(id)
            .filter(|entry| entry.observer.is_some())
            .ok_or_else(|| Box::new(unavailable(id, "prepared launcher is not registered")))?;
        entry
            .launcher
            .prepare_reserved(invocation, request, destination, cancellation)
    }
    /// Routes only failure notification to the existing prepared resource owner.
    /// The original owner must retain and validate custody; request data grants
    /// neither cancellation nor preparation/release authority. No fallback runs.
    pub fn preparation_acknowledgement_failed(
        &self,
        id: &str,
        invocation: &SandboxInvocation,
        request: &SandboxRequest,
        cause: &sim_lib_operation_gate::OperationError,
    ) -> std::result::Result<(), Box<SandboxRefusal>> {
        invocation.validate(id, request)?;
        let entry = self
            .0
            .get(id)
            .filter(|entry| entry.observer.is_some())
            .ok_or_else(|| Box::new(unavailable(id, "prepared launcher is not registered")))?;
        entry
            .launcher
            .preparation_acknowledgement_failed(invocation, request, cause)
    }
    /// Routes orphan-destination observation without allocation or release authority.
    pub fn observe_reservation(
        &self,
        id: &str,
        invocation: &SandboxInvocation,
        request: &SandboxRequest,
        destination: &Datum,
    ) -> std::result::Result<Datum, Box<SandboxRefusal>> {
        invocation.validate(id, request)?;
        let observer = self
            .0
            .get(id)
            .and_then(|entry| entry.observer.as_ref())
            .ok_or_else(|| Box::new(unavailable(id, "reservation observer is not registered")))?;
        observer.observe_reservation(invocation, request, destination)
    }
    /// Releases only through a registered prepared launcher, never direct launch.
    pub fn launch_prepared(
        &self,
        id: &str,
        invocation: &SandboxInvocation,
        request: &SandboxRequest,
        binding: &Datum,
        cancellation: &ProcessCancellation,
        admission: &mut dyn sim_lib_operation_gate::PreparedReleaseAdmission,
    ) -> SandboxAttempt {
        if let Err(refusal) = invocation.validate(id, request) {
            return SandboxAttempt::Unknown(*refusal);
        }
        match self.0.get(id).filter(|entry| entry.observer.is_some()) {
            Some(entry) if cancellation.is_cancelled() => {
                match entry
                    .launcher
                    .cancel_prepared(invocation, request, binding, cancellation)
                {
                    Ok(()) => SandboxAttempt::Unknown(unavailable(
                        id,
                        "cancelled preparation disposed; no payload completion is established",
                    )),
                    Err(refusal) => SandboxAttempt::Unknown(*refusal),
                }
            }
            Some(entry) => entry.launcher.launch_prepared(
                invocation,
                request,
                binding,
                cancellation,
                admission,
            ),
            None => SandboxAttempt::Unknown(unavailable(id, "prepared launcher is not registered")),
        }
    }
    /// Acquires custody from the separately registered independent observer.
    pub fn observe_prepared(
        &self,
        id: &str,
        invocation: &SandboxInvocation,
        request: &SandboxRequest,
        binding: &Datum,
        release: Option<&sim_kernel::ContentId>,
    ) -> std::result::Result<Box<dyn SandboxObservation>, Box<SandboxRefusal>> {
        invocation.validate(id, request)?;
        self.0
            .get(id)
            .and_then(|entry| entry.observer.as_ref())
            .ok_or_else(|| Box::new(unavailable(id, "prepared observer is not registered")))?
            .observe(invocation, request, binding, release)
    }
}

fn unavailable(id: &str, reason: &str) -> SandboxRefusal {
    SandboxRefusal {
        launcher: id.into(),
        reason: reason.into(),
        report: None,
    }
}

#[cfg(test)]
mod tests;
