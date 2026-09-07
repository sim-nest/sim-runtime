use std::fmt;

use sim_kernel::{CapabilityName, ContentId, Datum, NumberLiteral, Symbol};

use crate::{durable::*, operation_error::OperationError};

pub(crate) fn intent_datum(
    operation: &str,
    target: &Datum,
    intended_result: &Datum,
    replay_policy: ReplayPolicy,
) -> Datum {
    node(
        INTENT_TAG,
        vec![
            ("operation", Datum::String(operation.into())),
            ("target", target.clone()),
            ("intended-result", intended_result.clone()),
            ("replay-policy", replay_policy.datum()),
        ],
    )
}

pub(crate) fn grant_datum(
    operation: &OperationId,
    capability: &CapabilityName,
    authority: &Datum,
) -> Datum {
    node(
        GRANT_TAG,
        vec![
            ("operation", id_datum(&operation.0)),
            ("capability", Datum::String(capability.as_str().into())),
            ("authority", authority.clone()),
        ],
    )
}

pub(crate) fn attempt_datum(operation: &OperationId, ordinal: u64) -> Datum {
    node(
        ATTEMPT_TAG,
        vec![
            ("operation", id_datum(&operation.0)),
            (
                "ordinal",
                Datum::Number(NumberLiteral {
                    domain: Symbol::qualified("numbers", "u64"),
                    canonical: ordinal.to_string(),
                }),
            ),
        ],
    )
}

pub(crate) fn dispatch_datum(
    operation: &OperationId,
    grant: &OperationGrantId,
    attempt: &OperationAttemptId,
) -> Datum {
    node(
        DISPATCH_TAG,
        vec![
            ("operation", id_datum(&operation.0)),
            ("grant", id_datum(&grant.0)),
            ("attempt", id_datum(&attempt.0)),
        ],
    )
}

pub(crate) fn receipt_datum(dispatch: &DispatchId, raw: &Datum) -> Datum {
    node(
        RECEIPT_TAG,
        vec![("dispatch", id_datum(&dispatch.0)), ("raw", raw.clone())],
    )
}

pub(crate) fn node(tag: &'static str, fields: Vec<(&'static str, Datum)>) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("operation", tag),
        fields: fields
            .into_iter()
            .map(|(name, value)| (Symbol::new(name), value))
            .collect(),
    }
}

pub(crate) fn id_datum(id: &ContentId) -> Datum {
    node(
        "content-id-v1",
        vec![
            ("algorithm", Datum::Symbol(id.algorithm.clone())),
            ("digest", Datum::Bytes(id.bytes.to_vec())),
        ],
    )
}

pub(crate) fn id_from_datum(datum: &Datum) -> Result<ContentId, OperationError> {
    let fields = node_fields(datum, "content-id-v1", 2)?;
    let Datum::Symbol(algorithm) = field(fields, "algorithm")? else {
        return Err(OperationError::NonCanonical("content id algorithm"));
    };
    let Datum::Bytes(bytes) = field(fields, "digest")? else {
        return Err(OperationError::NonCanonical("content id digest"));
    };
    let bytes: [u8; 32] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| OperationError::NonCanonical("content id width"))?;
    let id = ContentId::from_bytes(algorithm.clone(), bytes);
    if id_datum(&id) != *datum {
        return Err(OperationError::NonCanonical("content id"));
    }
    Ok(id)
}

pub(crate) fn node_fields<'a>(
    datum: &'a Datum,
    tag: &'static str,
    count: usize,
) -> Result<&'a [(Symbol, Datum)], OperationError> {
    let Datum::Node {
        tag: observed,
        fields,
    } = datum
    else {
        return Err(OperationError::NonCanonical(tag));
    };
    if *observed != Symbol::qualified("operation", tag) || fields.len() != count {
        return Err(OperationError::NonCanonical(tag));
    }
    Ok(fields)
}

pub(crate) fn field<'a>(
    fields: &'a [(Symbol, Datum)],
    name: &str,
) -> Result<&'a Datum, OperationError> {
    let mut matches = fields
        .iter()
        .filter(|(key, _)| key.as_qualified_str() == name)
        .map(|(_, value)| value);
    let value = matches
        .next()
        .ok_or(OperationError::NonCanonical("missing field"))?;
    if matches.next().is_some() {
        return Err(OperationError::NonCanonical("duplicate field"));
    }
    Ok(value)
}
pub(crate) fn string_field<'a>(
    fields: &'a [(Symbol, Datum)],
    name: &str,
) -> Result<&'a str, OperationError> {
    match field(fields, name)? {
        Datum::String(value) => Ok(value),
        _ => Err(OperationError::NonCanonical("string field")),
    }
}

pub(crate) fn u64_field(fields: &[(Symbol, Datum)], name: &str) -> Result<u64, OperationError> {
    match field(fields, name)? {
        Datum::Number(NumberLiteral { domain, canonical })
            if *domain == Symbol::qualified("numbers", "u64") =>
        {
            canonical
                .parse()
                .map_err(|_| OperationError::NonCanonical("u64 field"))
        }
        _ => Err(OperationError::NonCanonical("u64 field")),
    }
}

pub(crate) fn content_id(datum: &Datum) -> Result<ContentId, OperationError> {
    datum
        .content_id()
        .map_err(|_| OperationError::NonCanonical("semantic datum"))
}

pub(crate) fn render_id(id: &ContentId, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(formatter, "{}:", id.algorithm.as_qualified_str())?;
    for byte in id.bytes {
        write!(formatter, "{byte:02x}")?;
    }
    Ok(())
}
