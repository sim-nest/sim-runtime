// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Portable checker-port request/result contract, independent of any
//! native process or path value.

use super::*;

/// Capability-scoped request naming only an installed allowlist entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalCheckRequest {
    packet: PacketRef,
    command: CommandId,
    source: BuildSourceRef,
    grant: CapabilityGrantRef,
    network_grant: Option<(CapabilityName, CapabilityGrantRef)>,
}

/// Explicit bounded lease request supplied to a local checker port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalCheckLease {
    /// Explicit clock epoch and units for both bounds; absence selects unscoped execution.
    /// A native checker compares this value with its boot-selected clock.
    pub clock: Option<Datum>,
    /// Stable holder identity.
    pub holder: Datum,
    /// Inclusive caller-supplied monotonic acquisition tick.
    pub acquired_at: u64,
    /// Exclusive caller-supplied monotonic expiry tick.
    pub expires_at: u64,
}

/// Portable projection of the durable operation outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalCheckStatus {
    /// The exact postcondition existed before dispatch.
    AlreadyTrue,
    /// An independent observer verified the postcondition after dispatch.
    Verified,
    /// An independent observer found a different postcondition.
    Diverged,
    /// Available facts cannot establish completion or safe replay.
    Uncertain,
    /// Admission or lifecycle validation refused the request.
    Refused,
}

/// Stable checker-facing response without native process or path values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalCheckResult {
    /// Semantic operation identity, when canonical admission succeeded.
    pub operation: Option<String>,
    /// Portable lifecycle outcome.
    pub status: LocalCheckStatus,
    /// Canonical outcome evidence or typed refusal detail.
    pub evidence: Datum,
}

/// Portable checker seam; packet tooling never constructs a native command.
pub trait LocalCheckPort: Send {
    /// Executes or reconciles one installed exact command under a bounded lease.
    fn check(
        &mut self,
        request: &LocalCheckRequest,
        lease: &LocalCheckLease,
        cancellation: &crate::ProcessCancellation,
    ) -> LocalCheckResult;
}

impl LocalCheckRequest {
    /// Creates a request that cannot alter the installed command bytes or policy.
    pub fn new(
        packet: PacketRef,
        command: CommandId,
        source: BuildSourceRef,
        grant: CapabilityGrantRef,
    ) -> Self {
        Self {
            packet,
            command,
            source,
            grant,
            network_grant: None,
        }
    }
    /// Adds authority for the exact separately scoped network capability.
    #[must_use]
    pub fn with_network_grant(
        mut self,
        capability: CapabilityName,
        grant: CapabilityGrantRef,
    ) -> Self {
        self.network_grant = Some((capability, grant));
        self
    }
    /// Returns the implementation packet identity.
    pub const fn packet(&self) -> &PacketRef {
        &self.packet
    }
    /// Returns the installed exact command identity.
    pub const fn command(&self) -> &CommandId {
        &self.command
    }
    /// Returns the sealed build-source identity.
    pub const fn source(&self) -> &BuildSourceRef {
        &self.source
    }
    /// Returns the least-authority grant identity.
    pub const fn grant(&self) -> &CapabilityGrantRef {
        &self.grant
    }
    /// Returns the separately scoped network capability and grant, when supplied.
    pub const fn network_grant(&self) -> Option<&(CapabilityName, CapabilityGrantRef)> {
        self.network_grant.as_ref()
    }
    /// Returns the request's canonical semantic value.
    pub fn canonical_datum(&self) -> Datum {
        node(
            "local-check-request-v1",
            vec![
                ("packet", Datum::String(self.packet.as_str().into())),
                ("command", id_datum(self.command.content_id())),
                ("source", Datum::String(self.source.as_str().into())),
                ("grant", Datum::String(self.grant.as_str().into())),
                (
                    "network-grant",
                    self.network_grant
                        .as_ref()
                        .map_or(Datum::Nil, |(capability, grant)| {
                            node(
                                "network-grant-v1",
                                vec![
                                    ("capability", Datum::String(capability.as_str().into())),
                                    ("grant", Datum::String(grant.as_str().into())),
                                ],
                            )
                        }),
                ),
            ],
        )
    }
}
