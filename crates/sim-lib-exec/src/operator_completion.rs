// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Authorized operator-exec completion.
//!
//! The platform capsule returns its own receipt. This record copies the argv,
//! environment, source, and authority of the request that was dispatched and
//! keeps that receipt's numeric status and bounded captures.

use crate::command_wire::environment_datum;
use crate::exec::{checked_request, result_datum};
use crate::{
    ArgAtom, ExecOptions, ProcessAttempt, ProcessCancellation, ProcessPort, ProcessReceipt,
    ProjectRootRef, SealedBindings, exec_capability,
};
use sim_kernel::{CapabilityName, Cx, Datum, Error, Result, Symbol};

/// Facts retained for one authorized operator-exec completion.
///
/// The capsule receipt is unchanged. `source` is the dispatched project-root
/// reference. `authority` is the capability required before dispatch.
pub(crate) struct OperatorExecCompletion {
    argv: Vec<ArgAtom>,
    environment: SealedBindings,
    source: ProjectRootRef,
    authority: CapabilityName,
    receipt: ProcessReceipt,
}

impl OperatorExecCompletion {
    /// Returns the capsule receipt.
    #[must_use]
    pub(crate) fn receipt(&self) -> &ProcessReceipt {
        &self.receipt
    }

    /// Returns the canonical record of this completion.
    #[must_use]
    pub(crate) fn canonical_datum(&self) -> Datum {
        Datum::Node {
            tag: Symbol::qualified("exec", "operator-exec-completion-v1"),
            fields: vec![
                (
                    Symbol::new("argv"),
                    Datum::Vector(
                        self.argv
                            .iter()
                            .map(|arg| Datum::String(arg.as_str().into()))
                            .collect(),
                    ),
                ),
                (
                    Symbol::new("environment"),
                    environment_datum(&self.environment),
                ),
                (
                    Symbol::new("source"),
                    Datum::String(self.source.as_str().into()),
                ),
                (
                    Symbol::new("authority"),
                    Datum::String(self.authority.as_str().into()),
                ),
                (
                    Symbol::new("provider"),
                    Datum::String(self.receipt.provider.clone()),
                ),
                (
                    Symbol::new("elapsed-monotonic-nanoseconds"),
                    Datum::String(self.receipt.elapsed_mono_ns.to_string()),
                ),
                (Symbol::new("result"), result_datum(&self.receipt.result)),
            ],
        }
    }
}

/// Requires `exec`, checks the request, and retains its facts on completion.
pub(crate) fn complete(
    cx: &mut Cx,
    port: &dyn ProcessPort,
    argv: &[String],
    options: &ExecOptions,
    cancellation: &ProcessCancellation,
) -> Result<OperatorExecCompletion> {
    cx.require(&exec_capability())?;
    let request = checked_request(argv, options)?;
    match port.run(&request, cancellation) {
        ProcessAttempt::Completed { receipt } => Ok(OperatorExecCompletion {
            argv: request.argv,
            environment: request.environment,
            source: request.root,
            authority: exec_capability(),
            receipt,
        }),
        attempt => Err(Error::HostError(format!("exec attempt: {attempt:?}"))),
    }
}
