//! Portable dispatch/command correlation, never native ownership or authority.

use crate::{CommandId, SandboxInvocation, command::request_wire};
use sim_kernel::{ContentId, Datum, Result};

/// Exact correlation shared by an installed worker and its subordinate entry.
/// Deserialization restores data only, not a lease, process or release seat.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SandboxInvocationBinding {
    /// Existing durable dispatch identity, including its algorithm.
    pub dispatch: ContentId,
    /// Exact command identity, including its algorithm.
    pub command: CommandId,
}

impl SandboxInvocationBinding {
    /// Decodes the existing correlation schema with strict fields and digest widths.
    /// Unknown algorithms remain distinct references and never alias known ones.
    pub fn from_datum(value: &Datum) -> Result<Self> {
        let fields = request_wire::fields(value, "sandbox-invocation-v1", 2)?;
        let dispatch = request_wire::content_id(request_wire::field(fields, "dispatch")?)?;
        let command = CommandId::from_binding_datum(request_wire::field(fields, "command")?)?;
        Ok(Self { dispatch, command })
    }

    /// Returns the existing exact dispatch/command wire value without command bytes.
    pub fn canonical_datum(&self) -> Datum {
        crate::command_wire::node(
            "sandbox-invocation-v1",
            vec![
                ("dispatch", crate::command_wire::id_datum(&self.dispatch)),
                (
                    "command",
                    crate::command_wire::id_datum(self.command.content_id()),
                ),
            ],
        )
    }
}

impl From<&SandboxInvocation> for SandboxInvocationBinding {
    fn from(invocation: &SandboxInvocation) -> Self {
        Self {
            dispatch: invocation.dispatch.clone(),
            command: invocation.command.id().clone(),
        }
    }
}
