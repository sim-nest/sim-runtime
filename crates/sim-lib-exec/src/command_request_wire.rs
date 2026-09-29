//! Strict decoding of references to boot-installed local checker commands.

use super::{BuildSourceRef, CapabilityGrantRef, CommandId, LocalCheckRequest, PacketRef};
use sim_kernel::{CapabilityName, ContentId, Datum, Error, Result, Symbol};

impl LocalCheckRequest {
    /// Decodes the exact semantic request format, independent of named-field order.
    ///
    /// This reconstructs references, not authority. The receiver must authenticate
    /// its caller, resolve the command against its boot-installed allowlist, and
    /// validate the named grants before accepting execution. No native path,
    /// executable bytes, resource policy or lease is accepted through this value.
    /// The enclosing transport owns its byte, nesting and message-count bounds.
    ///
    /// # Errors
    /// Refuses missing, duplicate, unknown or mistyped fields and malformed refs.
    pub fn from_datum(value: &Datum) -> Result<Self> {
        let fields = fields(value, "local-check-request-v1", 5)?;
        let packet = PacketRef::new(string(field(fields, "packet")?)?)?;
        let command = CommandId(content_id(field(fields, "command")?)?);
        let source = BuildSourceRef::new(string(field(fields, "source")?)?)?;
        let grant = CapabilityGrantRef::new(string(field(fields, "grant")?)?)?;
        let mut request = Self::new(packet, command, source, grant);
        match field(fields, "network-grant")? {
            Datum::Nil => {}
            value => {
                let fields = self::fields(value, "network-grant-v1", 2)?;
                let capability = string(field(fields, "capability")?)?;
                if capability.is_empty() || capability.contains('\0') {
                    return Err(invalid("network capability"));
                }
                let grant = CapabilityGrantRef::new(string(field(fields, "grant")?)?)?;
                request = request.with_network_grant(CapabilityName::new(capability), grant);
            }
        }
        Ok(request)
    }
}

impl CommandId {
    pub(crate) fn from_binding_datum(value: &Datum) -> Result<Self> {
        content_id(value).map(Self)
    }
}

pub(crate) fn content_id(value: &Datum) -> Result<ContentId> {
    let fields = fields(value, "content-id-v1", 2)?;
    let Datum::Symbol(algorithm) = field(fields, "algorithm")? else {
        return Err(invalid("command identity algorithm"));
    };
    let Datum::Bytes(bytes) = field(fields, "digest")? else {
        return Err(invalid("command identity digest"));
    };
    let bytes = bytes
        .as_slice()
        .try_into()
        .map_err(|_| invalid("command identity width"))?;
    // The algorithm is part of the reference. Only an exact installed command
    // can resolve it; an unknown algorithm never aliases an installed identity.
    Ok(ContentId::from_bytes(algorithm.clone(), bytes))
}

pub(crate) fn fields<'a>(
    value: &'a Datum,
    tag: &str,
    count: usize,
) -> Result<&'a [(Symbol, Datum)]> {
    let Datum::Node {
        tag: actual,
        fields,
    } = value
    else {
        return Err(invalid("node"));
    };
    if actual != &Symbol::qualified("local-check", tag) || fields.len() != count {
        return Err(invalid("node tag or field count"));
    }
    // Each caller requires every distinct known field. Exact cardinality plus
    // unique lookup rejects duplicate and unknown names without recursive hashing.
    Ok(fields)
}

pub(crate) fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Result<&'a Datum> {
    let key = Symbol::new(name);
    let mut matches = fields.iter().filter(|(candidate, _)| candidate == &key);
    let value = matches.next().ok_or_else(|| invalid("missing field"))?;
    if matches.next().is_some() {
        return Err(invalid("duplicate field"));
    }
    Ok(&value.1)
}

pub(super) fn string(value: &Datum) -> Result<&str> {
    match value {
        Datum::String(value) => Ok(value),
        _ => Err(invalid("string reference")),
    }
}

fn invalid(part: &str) -> Error {
    Error::Eval(format!("invalid local check request: {part}"))
}

#[cfg(test)]
#[path = "command_request_wire_tests.rs"]
mod tests;
