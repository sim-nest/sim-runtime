//! Read-eval decision records and ledger projection.

use std::sync::{Arc, Mutex, MutexGuard};

use sim_kernel::{
    CapabilityName, CapabilitySet, Cx, Datum, DatumStore, Error, Event, EventKind, EventLedger,
    Expr, Ref, Result, Symbol,
};

use super::RequestOrigin;

/// The outcome recorded for one explicit read-eval admission request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadEvalOutcome {
    /// The request passed every gate and its shape-checked result was admitted.
    Admitted,
    /// The trusted read policy did not grant `read-eval`.
    CapDenied,
    /// The read policy was untrusted for `read-eval`.
    TrustDenied,
    /// The caller lacked a capability listed in `requires`.
    MissingPower,
    /// The evaluated result failed the expected shape.
    ShapeDenied,
}

impl ReadEvalOutcome {
    fn as_symbol(&self) -> Symbol {
        match self {
            Self::Admitted => Symbol::new("admitted"),
            Self::CapDenied => Symbol::new("cap-denied"),
            Self::TrustDenied => Symbol::new("trust-denied"),
            Self::MissingPower => Symbol::new("missing-power"),
            Self::ShapeDenied => Symbol::new("shape-denied"),
        }
    }

    fn from_symbol(symbol: &Symbol) -> Result<Self> {
        match symbol.name.as_ref() {
            "admitted" => Ok(Self::Admitted),
            "cap-denied" => Ok(Self::CapDenied),
            "trust-denied" => Ok(Self::TrustDenied),
            "missing-power" => Ok(Self::MissingPower),
            "shape-denied" => Ok(Self::ShapeDenied),
            other => Err(Error::Eval(format!(
                "unknown read-eval decision outcome {other}"
            ))),
        }
    }
}

/// Data recorded for one explicit read-eval admission decision.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadEvalDecision {
    /// Open origin data describing who asked for eval.
    pub origin: RequestOrigin,
    /// Codec symbol used to decode the request source.
    pub codec: Symbol,
    /// Symbol naming the expected shape, when the shape exposes one.
    pub expected_shape: Option<Symbol>,
    /// Caller powers that were required before eval could run.
    pub requires: Vec<CapabilityName>,
    /// Maximum powers requested by the eval body (`allow`).
    pub requested: Vec<CapabilityName>,
    /// Powers that actually ran after diminishment.
    pub active: Vec<CapabilityName>,
    /// The gate outcome.
    pub outcome: ReadEvalOutcome,
}

/// Returns the default run reference used by the read-eval decision ledger.
pub fn read_eval_decision_run() -> Ref {
    Ref::Symbol(Symbol::qualified("read-eval", "decisions"))
}

#[derive(Clone)]
pub(super) struct ReadEvalLedger {
    events: Arc<Mutex<EventLedger>>,
    run: Ref,
}

impl Default for ReadEvalLedger {
    fn default() -> Self {
        Self {
            events: Arc::new(Mutex::new(EventLedger::new())),
            run: read_eval_decision_run(),
        }
    }
}

impl ReadEvalLedger {
    pub(super) fn record(&self, cx: &mut Cx, decision: &ReadEvalDecision) -> Result<Event> {
        let reference = decision_ref(cx, decision)?;
        self.lock()?
            .push(self.run.clone(), EventKind::Trace(reference))
    }

    pub(super) fn events_for_run(&self, run: &Ref) -> Result<Vec<Event>> {
        Ok(self.lock()?.events_for_run(run).to_vec())
    }

    pub(super) fn decisions(&self, cx: &Cx) -> Result<Vec<ReadEvalDecision>> {
        self.decisions_for_run(cx, &self.run)
    }

    pub(super) fn decisions_for_run(&self, cx: &Cx, run: &Ref) -> Result<Vec<ReadEvalDecision>> {
        self.events_for_run(run)?
            .iter()
            .filter_map(|event| match &event.kind {
                EventKind::Trace(reference) => Some(decision_from_ref(cx, reference)),
                _ => None,
            })
            .collect()
    }

    fn lock(&self) -> Result<MutexGuard<'_, EventLedger>> {
        self.events
            .lock()
            .map_err(|_| Error::PoisonedLock("read-eval decision ledger"))
    }
}

fn decision_ref(cx: &mut Cx, decision: &ReadEvalDecision) -> Result<Ref> {
    let id = cx.datum_store_mut().intern(decision_datum(decision))?;
    Ok(Ref::Content(id))
}

fn decision_from_ref(cx: &Cx, reference: &Ref) -> Result<ReadEvalDecision> {
    let Ref::Content(id) = reference else {
        return Err(Error::Eval(
            "read-eval decision trace does not reference content".to_owned(),
        ));
    };
    let datum = cx
        .datum_store()
        .get(id)?
        .ok_or_else(|| Error::Eval("read-eval decision content is missing".to_owned()))?;
    decision_from_datum(datum)
}

fn decision_datum(decision: &ReadEvalDecision) -> Datum {
    Datum::Node {
        tag: decision_tag(),
        fields: vec![
            (Symbol::new("origin"), origin_datum(&decision.origin)),
            (Symbol::new("codec"), Datum::Symbol(decision.codec.clone())),
            (
                Symbol::new("expected-shape"),
                option_symbol_datum(decision.expected_shape.as_ref()),
            ),
            (
                Symbol::new("requires"),
                capabilities_datum(&decision.requires),
            ),
            (
                Symbol::new("requested"),
                capabilities_datum(&decision.requested),
            ),
            (Symbol::new("active"), capabilities_datum(&decision.active)),
            (
                Symbol::new("outcome"),
                Datum::Symbol(decision.outcome.as_symbol()),
            ),
        ],
    }
}

fn decision_from_datum(datum: &Datum) -> Result<ReadEvalDecision> {
    let Datum::Node { tag, fields } = datum else {
        return Err(Error::Eval(
            "read-eval decision trace payload must be a datum node".to_owned(),
        ));
    };
    if tag != &decision_tag() {
        return Err(Error::Eval(
            "trace payload is not a read-eval decision".to_owned(),
        ));
    }
    Ok(ReadEvalDecision {
        origin: origin_from_datum(field(fields, "origin")?)?,
        codec: symbol_field(fields, "codec")?.clone(),
        expected_shape: option_symbol_from_datum(field(fields, "expected-shape")?)?,
        requires: capabilities_from_datum(field(fields, "requires")?)?,
        requested: capabilities_from_datum(field(fields, "requested")?)?,
        active: capabilities_from_datum(field(fields, "active")?)?,
        outcome: ReadEvalOutcome::from_symbol(symbol_field(fields, "outcome")?)?,
    })
}

fn origin_datum(origin: &RequestOrigin) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("read-eval", "origin"),
        fields: vec![
            (Symbol::new("tag"), Datum::Symbol(origin.tag.clone())),
            (
                Symbol::new("detail"),
                origin.detail.as_ref().map_or(Datum::Nil, expr_datum_lossy),
            ),
        ],
    }
}

fn origin_from_datum(datum: &Datum) -> Result<RequestOrigin> {
    let Datum::Node { tag, fields } = datum else {
        return Err(Error::Eval(
            "read-eval decision origin must be a datum node".to_owned(),
        ));
    };
    if tag != &Symbol::qualified("read-eval", "origin") {
        return Err(Error::Eval(
            "read-eval decision origin has the wrong tag".to_owned(),
        ));
    }
    let detail = match field(fields, "detail")? {
        Datum::Nil => None,
        other => Some(Expr::from(other.clone())),
    };
    Ok(RequestOrigin {
        tag: symbol_field(fields, "tag")?.clone(),
        detail,
    })
}

fn expr_datum_lossy(expr: &Expr) -> Datum {
    Datum::try_from(expr.clone()).unwrap_or_else(|_| Datum::String(format!("{expr:?}")))
}

fn capabilities_datum(capabilities: &[CapabilityName]) -> Datum {
    Datum::Vector(
        capabilities
            .iter()
            .map(|capability| Datum::String(capability.as_str().to_owned()))
            .collect(),
    )
}

fn capabilities_from_datum(datum: &Datum) -> Result<Vec<CapabilityName>> {
    let Datum::Vector(items) = datum else {
        return Err(Error::Eval(
            "read-eval decision capabilities must be a vector".to_owned(),
        ));
    };
    items
        .iter()
        .map(|item| match item {
            Datum::String(name) => Ok(CapabilityName::new(name.clone())),
            _ => Err(Error::Eval(
                "read-eval decision capability must be a string".to_owned(),
            )),
        })
        .collect()
}

fn option_symbol_datum(symbol: Option<&Symbol>) -> Datum {
    symbol.cloned().map_or(Datum::Nil, Datum::Symbol)
}

fn option_symbol_from_datum(datum: &Datum) -> Result<Option<Symbol>> {
    match datum {
        Datum::Nil => Ok(None),
        Datum::Symbol(symbol) => Ok(Some(symbol.clone())),
        _ => Err(Error::Eval(
            "read-eval decision optional symbol field is malformed".to_owned(),
        )),
    }
}

fn symbol_field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Result<&'a Symbol> {
    match field(fields, name)? {
        Datum::Symbol(symbol) => Ok(symbol),
        _ => Err(Error::Eval(format!(
            "read-eval decision field {name} must be a symbol"
        ))),
    }
}

fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Result<&'a Datum> {
    fields
        .iter()
        .find_map(|(field, value)| (field.name.as_ref() == name).then_some(value))
        .ok_or_else(|| Error::Eval(format!("read-eval decision missing {name} field")))
}

fn decision_tag() -> Symbol {
    Symbol::qualified("read-eval", "decision")
}

pub(super) fn decision_from_request(
    request: &super::ReadEvalRequest,
    active: &CapabilitySet,
    outcome: ReadEvalOutcome,
) -> ReadEvalDecision {
    ReadEvalDecision {
        origin: request.origin.clone(),
        codec: request.codec.clone(),
        expected_shape: request.expected_shape.symbol(),
        requires: request.requires.clone(),
        requested: request.allow.iter().cloned().collect(),
        active: active.iter().cloned().collect(),
        outcome,
    }
}
