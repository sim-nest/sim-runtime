// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Capability-gated bounded host-process execution for the SIM runtime.
//!
//! This crate supplies a general host `exec` operation for libraries that need
//! to run an external process under explicit authority. The operation accepts a
//! structured argv vector, never inserts a shell, captures stdout and stderr,
//! enforces a mandatory timeout, and truncates captured output at a caller-set
//! byte cap. [`CommandSpec`] identifies exact installed checker commands,
//! including unchanged interpreter bytes, resources, output and cleanup policy;
//! [`LocalCheckPort`] keeps packet tooling outside the native process boundary.
//! [`ExecServiceLib`] separates boot-selected administrative, worker and payload
//! entries with distinct capabilities. The host admits their concrete services;
//! loading a profile supplies neither native installation authority nor a grant.
//! [`ExecCommandLib`] exposes one exact host-bound ordinary request through
//! `exec/selected` and `cli/main/operator-exec`. It delegates to [`exec`] and
//! cannot replace the selected argv, options or environment. The zero-argument
//! selected callable is distinct from general agent `exec(argv, options)`.
//! It preserves raw process outcomes, not checker or sandbox qualification.
//! It is a host operation, not SIM evaluation.

mod command;
#[cfg(test)]
mod command_tests;
mod command_wire;
mod exec;
mod loaded;
mod local_check_reconciliation;
#[cfg(test)]
mod local_check_reconciliation_tests;
mod operator_args;
mod sandbox;
mod sandbox_binding;
#[cfg(test)]
mod sandbox_limit_tests;
mod sandbox_limits;
mod sandbox_prepared;
mod service;
pub use sandbox_binding::SandboxInvocationBinding;
mod sandbox_registry;

pub use command::{
    BuildSourceRef, CapabilityGrantRef, CheckoutFile, CleanupContract, CommandId,
    CommandInvocation, CommandReplayPolicy, CommandResource, CommandRoute, CommandSpec,
    LocalCheckLease, LocalCheckPort, LocalCheckRequest, LocalCheckResult, LocalCheckStatus,
    MAX_CHECKOUT_FILES, ManifestSelection, NetworkAccess, OutputContract, OutputExpectation,
    OutputState, PacketRef, ResourceAccess,
};

pub use exec::{
    ArgAtom, BindingValue, DispatchEvidence, ExecOptions, PrivateArtifactRef, ProcResult,
    ProcessAttempt, ProcessBudget, ProcessCancellation, ProcessPort, ProcessReceipt,
    ProcessRefusal, ProcessRequest, ProgramRef, ProjectRootRef, SealedBindings, StopReceipt, exec,
    exec_capability, proc_result_symbol,
};
pub use loaded::ExecCommandLib;
pub use local_check_reconciliation::{
    LocalCheckDurableReceipt, LocalCheckDurableState, LocalCheckJournalHead, LocalCheckPending,
    LocalCheckReconciliation, local_check_intent, project_local_check_outcome,
};
pub use operator_args::OperatorExecSelection;
pub use sandbox::{
    MountAccess, SandboxAttempt, SandboxControl, SandboxEvidence, SandboxLauncher, SandboxMount,
    SandboxPolicy, SandboxRefusal, SandboxReport, SandboxRequest, SandboxRequirement,
    SandboxResult, sandbox_exec,
};
pub use sandbox_limits::{
    SandboxCpuLimit, SandboxFilesystemLimit, SandboxLimits, SandboxMemoryLimit,
};
pub use sandbox_prepared::{
    RetainedOutputImage, SandboxCompilerSemanticReport, SandboxCompilerSemanticReportConsumer,
    SandboxCompilerSemanticReportCustody, SandboxInvocation, SandboxObservation, SandboxObserver,
};
pub use sandbox_registry::LauncherRegistry;
pub use service::{ExecServiceLib, ExecServicePort};

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
