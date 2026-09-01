//! Canonical lifecycle fact encoding and decoding.

use super::*;

pub(super) fn fact_kind(fact: &Fact) -> &'static str {
    match fact {
        Fact::Seal(_) => "seal",
        Fact::Claim { .. } => "claim",
        Fact::Terminal { .. } => "terminal",
        Fact::Retry { .. } => "retry",
        Fact::Interrupted { .. } => "interrupted",
    }
}
pub(super) fn state_for_outcome(value: AttemptOutcome) -> CoordinateState {
    match value {
        AttemptOutcome::Observed => CoordinateState::Observed,
        AttemptOutcome::Unsupported => CoordinateState::Unsupported,
        AttemptOutcome::Unresolved => CoordinateState::Unresolved,
        AttemptOutcome::Quarantined => CoordinateState::Quarantined,
    }
}
pub(super) fn outcome_code(value: AttemptOutcome) -> u8 {
    match value {
        AttemptOutcome::Observed => 1,
        AttemptOutcome::Unsupported => 2,
        AttemptOutcome::Unresolved => 3,
        AttemptOutcome::Quarantined => 4,
    }
}
pub(super) fn read_outcome(value: u8) -> Result<AttemptOutcome, LifecycleError> {
    match value {
        1 => Ok(AttemptOutcome::Observed),
        2 => Ok(AttemptOutcome::Unsupported),
        3 => Ok(AttemptOutcome::Unresolved),
        4 => Ok(AttemptOutcome::Quarantined),
        _ => Err(LifecycleError::CorruptFact),
    }
}
pub(super) fn object_id(bytes: &[u8]) -> ContentId {
    ContentId::from_bytes(
        Symbol::qualified("study", "sha256-seal-v1"),
        Sha256::digest(bytes).into(),
    )
}
pub(super) fn claim_id(
    study: &ContentId,
    coordinate: &ContentId,
    revision: u32,
    fence: u64,
) -> ContentId {
    let mut bytes = Vec::new();
    put_id(&mut bytes, study);
    put_id(&mut bytes, coordinate);
    put_u32(&mut bytes, revision);
    bytes.extend_from_slice(&fence.to_be_bytes());
    object_id(&bytes)
}

pub(super) fn encode_seal_body(
    coordinates: &[StudyCoordinate],
    policy: &SealPolicy,
) -> Result<Vec<u8>, LifecycleError> {
    let mut out = vec![FORMAT];
    put_u32(&mut out, coordinates.len() as u32);
    for coordinate in coordinates {
        put_coordinate(&mut out, coordinate);
    }
    put_id(&mut out, &policy.selection_policy);
    put_id(&mut out, &policy.source.revision);
    put_u8(&mut out, u8::from(policy.source.clean));
    put_u32(&mut out, policy.bounds.max_coordinates);
    put_u32(&mut out, policy.bounds.max_attempts_per_coordinate);
    put_id(&mut out, &policy.invocation);
    put_closure(&mut out, &policy.closure);
    Ok(out)
}
pub(super) fn encode_fact(fact: &Fact) -> Result<Vec<u8>, LifecycleError> {
    let mut out = vec![FORMAT];
    match fact {
        Fact::Seal(study) => {
            put_u8(&mut out, 1);
            let body = encode_seal_body(&study.coordinates, &study.policy)?;
            put_bytes(&mut out, &body);
        }
        Fact::Claim {
            study,
            coordinate,
            revision,
            claim,
        } => {
            put_u8(&mut out, 2);
            put_id(&mut out, study);
            put_id(&mut out, coordinate);
            put_u32(&mut out, *revision);
            put_id(&mut out, claim);
        }
        Fact::Terminal {
            study,
            coordinate,
            revision,
            claim,
            outcome,
            discarded,
        } => {
            put_u8(&mut out, 3);
            put_id(&mut out, study);
            put_id(&mut out, coordinate);
            put_u32(&mut out, *revision);
            put_id(&mut out, claim);
            put_u8(&mut out, outcome_code(*outcome));
            put_u8(&mut out, u8::from(*discarded));
        }
        Fact::Retry {
            study,
            coordinate,
            next_revision,
        } => {
            put_u8(&mut out, 4);
            put_id(&mut out, study);
            put_id(&mut out, coordinate);
            put_u32(&mut out, *next_revision);
        }
        Fact::Interrupted {
            study,
            coordinate,
            revision,
            claim,
        } => {
            put_u8(&mut out, 5);
            put_id(&mut out, study);
            put_id(&mut out, coordinate);
            put_u32(&mut out, *revision);
            put_id(&mut out, claim);
        }
    }
    Ok(out)
}
pub(super) fn decode_fact(bytes: &[u8]) -> Result<Fact, LifecycleError> {
    let mut r = Reader::new(bytes);
    if r.u8()? != FORMAT {
        return Err(LifecycleError::CorruptFact);
    }
    let fact = match r.u8()? {
        1 => {
            let body = r.bytes()?;
            let (coordinates, policy) = decode_seal_body(&body)?;
            Fact::Seal(Box::new(SealedStudy::new(coordinates, policy)?))
        }
        2 => Fact::Claim {
            study: r.id()?,
            coordinate: r.id()?,
            revision: r.u32()?,
            claim: r.id()?,
        },
        3 => Fact::Terminal {
            study: r.id()?,
            coordinate: r.id()?,
            revision: r.u32()?,
            claim: r.id()?,
            outcome: read_outcome(r.u8()?)?,
            discarded: r.u8()? != 0,
        },
        4 => Fact::Retry {
            study: r.id()?,
            coordinate: r.id()?,
            next_revision: r.u32()?,
        },
        5 => Fact::Interrupted {
            study: r.id()?,
            coordinate: r.id()?,
            revision: r.u32()?,
            claim: r.id()?,
        },
        _ => return Err(LifecycleError::CorruptFact),
    };
    if !r.done() {
        return Err(LifecycleError::CorruptFact);
    }
    Ok(fact)
}
pub(super) fn decode_seal_body(
    bytes: &[u8],
) -> Result<(Vec<StudyCoordinate>, SealPolicy), LifecycleError> {
    let mut r = Reader::new(bytes);
    if r.u8()? != FORMAT {
        return Err(LifecycleError::CorruptFact);
    }
    let count = r.u32()?;
    let mut coordinates = Vec::new();
    for _ in 0..count {
        coordinates.push(r.coordinate()?);
    }
    let selection_policy = r.id()?;
    let source = SourceAssertion {
        revision: r.id()?,
        clean: r.u8()? != 0,
    };
    let bounds = StudyBounds {
        max_coordinates: r.u32()?,
        max_attempts_per_coordinate: r.u32()?,
    };
    let invocation = r.id()?;
    let closure = r.closure()?;
    if !r.done() {
        return Err(LifecycleError::CorruptFact);
    }
    Ok((
        coordinates,
        SealPolicy {
            selection_policy,
            source,
            bounds,
            invocation,
            closure,
        },
    ))
}
pub(super) fn put_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}
pub(super) fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}
pub(super) fn put_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    put_u32(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
}
pub(super) fn put_symbol(out: &mut Vec<u8>, value: &Symbol) {
    put_bytes(out, value.as_qualified_str().as_bytes());
}
pub(super) fn put_id(out: &mut Vec<u8>, value: &ContentId) {
    put_symbol(out, &value.algorithm);
    out.extend_from_slice(&value.bytes);
}
pub(super) fn put_coordinate(out: &mut Vec<u8>, value: &StudyCoordinate) {
    put_id(out, value.subject().content_id());
    put_id(out, value.task());
    put_id(out, value.harness());
    put_id(out, value.request());
    put_id(out, value.treatment());
    put_u32(out, value.sample_index());
}
pub(super) fn put_closure(out: &mut Vec<u8>, value: &RequiredClosure) {
    put_id(out, &value.task);
    put_id(out, &value.harness);
    put_id(out, &value.request);
    put_id(out, &value.grader);
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], LifecycleError> {
        let end = self
            .at
            .checked_add(count)
            .ok_or(LifecycleError::CorruptFact)?;
        let value = self
            .bytes
            .get(self.at..end)
            .ok_or(LifecycleError::CorruptFact)?;
        self.at = end;
        Ok(value)
    }
    fn u8(&mut self) -> Result<u8, LifecycleError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, LifecycleError> {
        Ok(u32::from_be_bytes(
            self.take(4)?
                .try_into()
                .map_err(|_| LifecycleError::CorruptFact)?,
        ))
    }
    fn bytes(&mut self) -> Result<Vec<u8>, LifecycleError> {
        let len = self.u32()? as usize;
        Ok(self.take(len)?.to_vec())
    }
    fn id(&mut self) -> Result<ContentId, LifecycleError> {
        let symbol = String::from_utf8(self.bytes()?).map_err(|_| LifecycleError::CorruptFact)?;
        let (namespace, name) = symbol.split_once('/').ok_or(LifecycleError::CorruptFact)?;
        let mut digest = [0; 32];
        digest.copy_from_slice(self.take(32)?);
        Ok(ContentId::from_bytes(
            Symbol::qualified(namespace, name),
            digest,
        ))
    }
    fn coordinate(&mut self) -> Result<StudyCoordinate, LifecycleError> {
        Ok(StudyCoordinate::new(
            SubjectRevision::new(self.id()?),
            self.id()?,
            self.id()?,
            self.id()?,
            self.id()?,
            self.u32()?,
        ))
    }
    fn closure(&mut self) -> Result<RequiredClosure, LifecycleError> {
        Ok(RequiredClosure {
            task: self.id()?,
            harness: self.id()?,
            request: self.id()?,
            grader: self.id()?,
        })
    }
    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}
