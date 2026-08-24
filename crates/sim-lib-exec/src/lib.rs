#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Capability-gated bounded host-process execution for the SIM runtime.
//!
//! This crate supplies a general host `exec` operation for libraries that need
//! to run an external process under explicit authority. The operation accepts a
//! structured argv vector, never inserts a shell, captures stdout and stderr,
//! enforces a mandatory timeout, and truncates captured output at a caller-set
//! byte cap. It is a host operation, not SIM evaluation.

mod exec;
mod sandbox;

pub use exec::{
    ArgAtom, BindingValue, DispatchEvidence, ExecOptions, PrivateArtifactRef, ProcResult,
    ProcessAttempt, ProcessBudget, ProcessCancellation, ProcessPort, ProcessReceipt,
    ProcessRefusal, ProcessRequest, ProgramRef, ProjectRootRef, SealedBindings, StopReceipt, exec,
    exec_capability, proc_result_symbol,
};
pub use sandbox::{
    LauncherRegistry, MountAccess, SandboxAttempt, SandboxControl, SandboxEvidence,
    SandboxLauncher, SandboxLimits, SandboxMount, SandboxPolicy, SandboxRefusal, SandboxReport,
    SandboxRequest, SandboxRequirement, SandboxResult, sandbox_exec,
};

/// Cookbook recipes for this lib, embedded at build time.
pub static RECIPES: sim_cookbook::EmbeddedDir =
    include!(concat!(env!("OUT_DIR"), "/cookbook_recipes.rs"));

#[cfg(test)]
mod tests;
