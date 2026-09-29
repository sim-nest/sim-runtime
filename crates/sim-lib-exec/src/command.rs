// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Exact, content-identified local checker command contracts.

use std::{collections::BTreeSet, fmt};

use sim_kernel::{CapabilityName, ContentId, Datum, Error, Result, Symbol};

#[path = "command_checkout.rs"]
mod checkout;
#[path = "command_decode.rs"]
mod decode;
#[path = "command_local_check.rs"]
mod local_check;
pub use local_check::{
    LocalCheckLease, LocalCheckPort, LocalCheckRequest, LocalCheckResult, LocalCheckStatus,
};
#[path = "command_request_wire.rs"]
pub(crate) mod request_wire;

use crate::{
    ArgAtom, ProcessBudget, ProgramRef, ProjectRootRef, SandboxControl, SandboxPolicy,
    SandboxRequirement, SealedBindings,
    command_wire::{
        budget_datum, environment_datum, i64_datum, id_datum, invocation_datum, network_datum,
        node, output_datum, replay_datum, resource_datum, route_datum,
    },
};

macro_rules! opaque_ref {
    ($name:ident, $doc:literal, $label:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(String);
        impl $name {
            /// Validates a stable, non-native reference.
            pub fn new(value: impl Into<String>) -> Result<Self> {
                let value = value.into();
                if value.is_empty() || value.contains('\0') {
                    return Err(Error::Eval(
                        concat!($label, " must be non-empty and NUL-free").into(),
                    ));
                }
                Ok(Self(value))
            }
            /// Returns the stable reference string.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

opaque_ref!(
    PacketRef,
    "Stable implementation packet reference.",
    "packet reference"
);
opaque_ref!(
    BuildSourceRef,
    "Stable sealed build-source reference.",
    "build-source reference"
);
opaque_ref!(
    CapabilityGrantRef,
    "Stable least-authority grant reference.",
    "capability-grant reference"
);

/// Semantic identity of an exact trusted command specification.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CommandId(ContentId);

impl CommandId {
    /// Borrows the command's semantic content identity.
    pub const fn content_id(&self) -> &ContentId {
        &self.0
    }
}

impl fmt::Display for CommandId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:", self.0.algorithm.as_qualified_str())?;
        for byte in self.0.bytes {
            write!(formatter, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Replay law bound into the local request without depending on the operation crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandReplayPolicy {
    /// A repeat may occur only after independent absence and lease expiry.
    Idempotent,
    /// A recorded dispatch is an unconditional at-most-once barrier.
    ExactlyOnce,
}

/// Exact executable invocation; no variant performs shell parsing of argv.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandInvocation {
    /// Invoke the allowlisted program with whole literal arguments.
    Argv(Vec<ArgAtom>),
    /// Invoke an allowlisted interpreter with flags and unchanged trusted script bytes.
    Interpreter {
        /// Whole literal interpreter flags preceding the script.
        flags: Vec<ArgAtom>,
        /// Exact UTF-8, NUL-free script bytes included in [`CommandId`].
        script: Vec<u8>,
    },
}

impl CommandInvocation {
    /// Renders whole argument atoms without interpolation or splitting.
    pub fn argv(&self) -> Result<Vec<ArgAtom>> {
        match self {
            Self::Argv(argv) => Ok(argv.clone()),
            Self::Interpreter { flags, script } => {
                let script = std::str::from_utf8(script)
                    .map_err(|_| Error::Eval("trusted command script is not UTF-8".into()))?;
                if script.contains('\0') {
                    return Err(Error::Eval("trusted command script contains NUL".into()));
                }
                let mut argv = flags.clone();
                argv.push(ArgAtom::new(script)?);
                Ok(argv)
            }
        }
    }
}

/// Read or write authority for one boot-resolved command resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResourceAccess {
    /// Immutable command input.
    ReadOnly,
    /// Explicit command output, cache, or scratch root.
    Writable,
}

/// One opaque, boot-resolved resource made available to an exact command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandResource {
    /// Stable boot configuration identity.
    pub source: String,
    /// Absolute guest path when sandboxed.
    pub guest_path: String,
    /// Exact access authority.
    pub access: ResourceAccess,
}

/// One exact input file materialized into the writable working root.
///
/// Before the command may run, its owner copies the bytes at `path` inside the
/// declared read-only resource `resource` to `target` inside the command's
/// writable working root. Both paths are canonical relative paths. The copy is
/// part of the command identity; it is how an owned disposable checkout gets
/// its exact inputs without granting the payload write access to the source.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CheckoutFile {
    /// Declared read-only resource holding the input.
    pub resource: String,
    /// Relative path of the input inside that resource.
    pub path: String,
    /// Relative path of the copy inside the writable working root.
    pub target: String,
}

/// Largest number of files one command checkout may materialize.
pub const MAX_CHECKOUT_FILES: usize = 16;

/// Declares that an interpreter command's script is one field of an owner
/// manifest held in a declared read-only resource.
///
/// The manifest at `path` is a table of entries; the entry in array `table`
/// whose `name` key equals `name` has a string `field` whose exact bytes are
/// the command's script. The selection is part of the command identity, and
/// the owner that runs the command proves the equality from the pinned
/// manifest before execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestSelection {
    /// Declared read-only resource holding the manifest.
    pub resource: String,
    /// Relative path of the manifest inside that resource.
    pub path: String,
    /// Array of entries inside the manifest, for example `repo`.
    pub table: String,
    /// Value of the selected entry's `name` key.
    pub name: String,
    /// String field holding the script, for example `validation_command`.
    pub field: String,
}

/// Expected state of one declared output path after execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OutputState {
    /// The path must exist, regardless of content.
    Exists,
    /// The path must be absent.
    Absent,
    /// The file's `Datum::Bytes` semantic identity must equal this value.
    FileContent(ContentId),
}

/// One independently observable path beneath a writable resource.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputExpectation {
    /// Writable resource identity containing the path.
    pub resource: String,
    /// Slash-separated relative path without parent traversal.
    pub relative_path: String,
    /// Exact expected state.
    pub state: OutputState,
}

/// Semantic process result and filesystem postcondition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputContract {
    exit_codes: BTreeSet<i32>,
    outputs: Vec<OutputExpectation>,
}

impl OutputContract {
    /// Validates a non-empty exit set and traversal-free unique output paths.
    pub fn new(
        exit_codes: impl IntoIterator<Item = i32>,
        outputs: Vec<OutputExpectation>,
    ) -> Result<Self> {
        let exit_codes = exit_codes.into_iter().collect::<BTreeSet<_>>();
        if exit_codes.is_empty() {
            return Err(Error::Eval("output contract needs an exit code".into()));
        }
        let mut paths = BTreeSet::new();
        for output in &outputs {
            if output.resource.is_empty()
                || output.relative_path.is_empty()
                || output.relative_path.starts_with('/')
                || output
                    .relative_path
                    .split('/')
                    .any(|part| part.is_empty() || part == "." || part == "..")
                || output.relative_path.contains('\0')
                || !paths.insert((&output.resource, &output.relative_path))
            {
                return Err(Error::Eval(
                    "invalid or duplicate output expectation".into(),
                ));
            }
        }
        Ok(Self {
            exit_codes,
            outputs,
        })
    }
    /// Returns accepted native exit codes.
    pub fn exit_codes(&self) -> &BTreeSet<i32> {
        &self.exit_codes
    }
    /// Returns independently observable output expectations.
    pub fn outputs(&self) -> &[OutputExpectation] {
        &self.outputs
    }
    /// Returns the exact semantic output-contract value.
    pub fn canonical_datum(&self) -> Datum {
        node(
            "output-contract-v1",
            vec![
                (
                    "exit-codes",
                    Datum::Set(
                        self.exit_codes
                            .iter()
                            .map(|value| i64_datum(i64::from(*value)))
                            .collect(),
                    ),
                ),
                (
                    "outputs",
                    Datum::Vector(self.outputs.iter().map(output_datum).collect()),
                ),
            ],
        )
    }
}

/// Required cleanup proof for process descendants and writable scratch roots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CleanupContract {
    scratch_resources: BTreeSet<String>,
}

impl CleanupContract {
    /// Requires group kill/reap and names every disposable writable scratch root.
    pub fn process_group(scratch_resources: impl IntoIterator<Item = String>) -> Result<Self> {
        let scratch_resources = scratch_resources.into_iter().collect::<BTreeSet<_>>();
        if scratch_resources
            .iter()
            .any(|value| value.is_empty() || value.contains('\0'))
        {
            return Err(Error::Eval("invalid cleanup resource".into()));
        }
        Ok(Self { scratch_resources })
    }
    /// Returns disposable writable resources that must be empty or removed at closure.
    pub fn scratch_resources(&self) -> &BTreeSet<String> {
        &self.scratch_resources
    }
    /// Returns the semantic cleanup contract.
    pub fn canonical_datum(&self) -> Datum {
        node(
            "cleanup-contract-v1",
            vec![
                (
                    "descendant-group",
                    Datum::Symbol(Symbol::qualified("cleanup", "kill-reap-required")),
                ),
                (
                    "scratch-resources",
                    Datum::Set(
                        self.scratch_resources
                            .iter()
                            .cloned()
                            .map(Datum::String)
                            .collect(),
                    ),
                ),
            ],
        )
    }
}

/// Network authority for one command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkAccess {
    /// No network namespace or socket authority is available.
    Absent,
    /// A separately supplied capability authorizes this exact network use.
    Scoped(CapabilityName),
}

/// Selected capsule execution boundary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandRoute {
    /// Exact trusted command in an owned disposable host checkout.
    Process,
    /// Networkless anonymous-root execution through a registered launcher.
    Sandbox {
        /// Stable registered launcher identity.
        launcher: String,
        /// Complete sandbox authority and resource bounds.
        policy: SandboxPolicy,
    },
}

/// Immutable allowlist entry pinning every local checker execution input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    id: CommandId,
    program: ProgramRef,
    root: ProjectRootRef,
    invocation: CommandInvocation,
    environment: SealedBindings,
    resources: Vec<CommandResource>,
    budget: ProcessBudget,
    outputs: OutputContract,
    cleanup: CleanupContract,
    network: NetworkAccess,
    route: CommandRoute,
    replay: CommandReplayPolicy,
    checkout: Vec<CheckoutFile>,
    manifest: Option<ManifestSelection>,
}

impl CommandSpec {
    /// Validates and identifies an exact trusted command allowlist entry.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        program: ProgramRef,
        root: ProjectRootRef,
        invocation: CommandInvocation,
        environment: SealedBindings,
        resources: Vec<CommandResource>,
        budget: ProcessBudget,
        outputs: OutputContract,
        cleanup: CleanupContract,
        network: NetworkAccess,
        route: CommandRoute,
        replay: CommandReplayPolicy,
    ) -> Result<Self> {
        invocation.argv()?;
        if budget.timeout_ms == 0 || budget.max_output_bytes == 0 {
            return Err(Error::Eval("command budget must be non-zero".into()));
        }
        if matches!(invocation, CommandInvocation::Interpreter { .. }) && budget.stdin.is_some() {
            return Err(Error::Eval(
                "interpreter command reserves no second stdin script channel".into(),
            ));
        }
        let mut sources = BTreeSet::new();
        let mut guests = BTreeSet::new();
        for resource in &resources {
            if resource.source.is_empty()
                || !resource.guest_path.starts_with('/')
                || resource.guest_path.split('/').any(|part| part == "..")
                || resource.guest_path.contains('\0')
                || !sources.insert(resource.source.as_str())
                || !guests.insert(resource.guest_path.as_str())
            {
                return Err(Error::Eval("invalid or duplicate command resource".into()));
            }
        }
        let writable = resources
            .iter()
            .filter(|resource| resource.access == ResourceAccess::Writable)
            .map(|resource| resource.source.as_str())
            .collect::<BTreeSet<_>>();
        if outputs
            .outputs
            .iter()
            .any(|output| !writable.contains(output.resource.as_str()))
            || cleanup
                .scratch_resources
                .iter()
                .any(|resource| !writable.contains(resource.as_str()))
        {
            return Err(Error::Eval(
                "outputs and cleanup must name declared writable resources".into(),
            ));
        }
        if let CommandRoute::Sandbox { launcher, policy } = &route {
            if launcher.is_empty() {
                return Err(Error::Eval("sandbox launcher identity is empty".into()));
            }
            let mounts = policy
                .mounts()
                .iter()
                .map(|mount| {
                    (
                        &mount.source,
                        &mount.guest_path,
                        match mount.access {
                            crate::MountAccess::ReadOnly => ResourceAccess::ReadOnly,
                            crate::MountAccess::Writable => ResourceAccess::Writable,
                        },
                    )
                })
                .collect::<BTreeSet<_>>();
            let declared = resources
                .iter()
                .map(|resource| (&resource.source, &resource.guest_path, resource.access))
                .collect::<BTreeSet<_>>();
            if mounts != declared {
                return Err(Error::Eval(
                    "sandbox mounts differ from command resources".into(),
                ));
            }
            if !resources
                .iter()
                .any(|resource| resource.source == root.as_str() && resource.guest_path == "/work")
            {
                return Err(Error::Eval(
                    "sandbox working root is not the declared /work resource".into(),
                ));
            }
            if policy.limits().wall_time_ms != budget.timeout_ms
                || policy.limits().output_bytes != budget.max_output_bytes
                || budget
                    .stdin
                    .as_ref()
                    .is_some_and(|stdin| stdin.len() > policy.limits().stdin_bytes)
            {
                return Err(Error::Eval(
                    "sandbox and command process budgets differ".into(),
                ));
            }
            if !matches!(network, NetworkAccess::Absent)
                || policy.requirements().get(&SandboxControl::Network)
                    != Some(&SandboxRequirement::Required)
            {
                return Err(Error::Eval(
                    "current sandbox route requires proven absent networking".into(),
                ));
            }
        } else if matches!(network, NetworkAccess::Absent) {
            return Err(Error::Eval(
                "host process route cannot prove absent networking".into(),
            ));
        }
        let mut value = Self {
            id: CommandId(ContentId::from_bytes(
                Symbol::qualified("core", "sha256-datum-v1"),
                [0; 32],
            )),
            program,
            root,
            invocation,
            environment,
            resources,
            budget,
            outputs,
            cleanup,
            network,
            route,
            replay,
            checkout: Vec::new(),
            manifest: None,
        };
        value.id = CommandId(
            value
                .canonical_without_id()
                .content_id()
                .map_err(|_| Error::Eval("command specification is not canonical".into()))?,
        );
        Ok(value)
    }
    /// Returns the stable command identity.
    pub const fn id(&self) -> &CommandId {
        &self.id
    }
    /// Returns the boot-trusted executable or interpreter identity.
    pub const fn program(&self) -> &ProgramRef {
        &self.program
    }
    /// Returns the boot-trusted working-root identity.
    pub const fn root(&self) -> &ProjectRootRef {
        &self.root
    }
    /// Returns the exact invocation.
    pub const fn invocation(&self) -> &CommandInvocation {
        &self.invocation
    }
    /// Returns the sealed, empty-by-default environment.
    pub const fn environment(&self) -> &SealedBindings {
        &self.environment
    }
    /// Returns all explicit input, output, cache, and scratch resources.
    pub fn resources(&self) -> &[CommandResource] {
        &self.resources
    }
    /// Returns the mandatory time, input, and output budget.
    pub const fn budget(&self) -> &ProcessBudget {
        &self.budget
    }
    /// Returns the semantic postcondition contract.
    pub const fn outputs(&self) -> &OutputContract {
        &self.outputs
    }
    /// Returns the descendant and resource cleanup contract.
    pub const fn cleanup(&self) -> &CleanupContract {
        &self.cleanup
    }
    /// Returns the separately scoped network policy.
    pub const fn network(&self) -> &NetworkAccess {
        &self.network
    }
    /// Returns the selected process or sandbox boundary.
    pub const fn route(&self) -> &CommandRoute {
        &self.route
    }
    /// Returns the replay policy bound into operation intent.
    pub const fn replay(&self) -> CommandReplayPolicy {
        self.replay
    }
    /// Returns the exact input files materialized before execution.
    pub fn checkout(&self) -> &[CheckoutFile] {
        &self.checkout
    }
    /// Returns the owner manifest field this command's script must equal.
    pub const fn manifest(&self) -> Option<&ManifestSelection> {
        self.manifest.as_ref()
    }
    /// Returns the canonical semantic command specification.
    pub fn canonical_datum(&self) -> Datum {
        self.canonical_without_id()
    }
    fn canonical_without_id(&self) -> Datum {
        let mut fields = vec![
            ("program", Datum::String(self.program.as_str().into())),
            ("root", Datum::String(self.root.as_str().into())),
            ("invocation", invocation_datum(&self.invocation)),
            ("environment", environment_datum(&self.environment)),
            (
                "resources",
                Datum::Vector(self.resources.iter().map(resource_datum).collect()),
            ),
            ("budget", budget_datum(&self.budget)),
            ("outputs", self.outputs.canonical_datum()),
            ("cleanup", self.cleanup.canonical_datum()),
            ("network", network_datum(&self.network)),
            ("route", route_datum(&self.route)),
            ("replay", replay_datum(self.replay)),
        ];
        // A command without a checkout or manifest keeps its original v1 identity.
        if self.checkout.is_empty() && self.manifest.is_none() {
            return node("command-spec-v1", fields);
        }
        fields.push((
            "checkout",
            Datum::Vector(self.checkout.iter().map(checkout_datum).collect()),
        ));
        fields.push((
            "manifest",
            self.manifest.as_ref().map_or(Datum::Nil, manifest_datum),
        ));
        node("command-spec-v2", fields)
    }
}

fn checkout_datum(file: &CheckoutFile) -> Datum {
    node(
        "checkout-file-v1",
        vec![
            ("resource", Datum::String(file.resource.clone())),
            ("path", Datum::String(file.path.clone())),
            ("target", Datum::String(file.target.clone())),
        ],
    )
}

fn manifest_datum(selection: &ManifestSelection) -> Datum {
    node(
        "manifest-selection-v1",
        vec![
            ("resource", Datum::String(selection.resource.clone())),
            ("path", Datum::String(selection.path.clone())),
            ("table", Datum::String(selection.table.clone())),
            ("name", Datum::String(selection.name.clone())),
            ("field", Datum::String(selection.field.clone())),
        ],
    )
}

pub(crate) fn canonical_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.starts_with('/')
        && !path
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}
