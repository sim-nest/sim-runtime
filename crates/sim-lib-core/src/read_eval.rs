//! Explicit read-eval admission through one diminished gate.

mod config;
mod decision;

use std::sync::Arc;

use sim_codec::{Input, decode_with_codec};
use sim_kernel::{
    AbiVersion, CapabilityName, CapabilitySet, Cx, Diagnostic, Error, Event, Export, Expr, Lib,
    LibManifest, LibTarget, Linker, LoadCx, Object, ReadPolicy, Ref, Result, Shape, ShapeId,
    Symbol, Value, Version, read_eval_capability,
};
use sim_shape::expected_shape_diagnostic;

pub use config::{
    ConfigEvalNode, HostConfigEvalOptIn, config_eval_node_symbol, config_eval_origin_tag,
    parse_config_eval_node, realize_config_expr,
};
pub use decision::{ReadEvalDecision, ReadEvalOutcome, read_eval_decision_run};

/// Open origin data for an explicit read-eval request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestOrigin {
    /// Open tag for the request origin, such as `config/node` or `repl`.
    pub tag: Symbol,
    /// Optional origin detail carried as data for callers and ledger records.
    pub detail: Option<Expr>,
}

impl RequestOrigin {
    /// Builds origin data from a tag with no detail.
    pub fn new(tag: Symbol) -> Self {
        Self { tag, detail: None }
    }

    /// Builds origin data from a tag and detail expression.
    pub fn with_detail(tag: Symbol, detail: Expr) -> Self {
        Self {
            tag,
            detail: Some(detail),
        }
    }
}

/// Source accepted by the read-eval broker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadEvalSource {
    /// Decode this text through the request codec before evaluation.
    Text(String),
    /// Decode these bytes through the request codec before evaluation.
    Bytes(Vec<u8>),
    /// Evaluate an already-decoded expression.
    Expr(Expr),
}

/// A single explicit, host-authorized read-eval admission request.
pub struct ReadEvalRequest {
    /// Open origin data describing who asked for eval.
    pub origin: RequestOrigin,
    /// Codec symbol used to decode text or bytes sources.
    pub codec: Symbol,
    /// Source to decode and evaluate, or an already-decoded expression.
    pub source: ReadEvalSource,
    /// Trusted host-built read policy; never derive this from request text.
    pub read_policy: ReadPolicy,
    /// Capabilities the caller must already hold before eval can run.
    pub requires: Vec<CapabilityName>,
    /// Maximum powers the request allows the eval body to run with.
    pub allow: CapabilitySet,
    /// Shape the evaluated result must satisfy before it is admitted.
    pub expected_shape: Arc<dyn Shape>,
}

// sim-non-citizen(reason = "host admission gate object; explicit request data is not a read-constructor surface", kind = "runtime", descriptor = "")
/// The one runtime admission gate for explicit diminished read-eval.
#[derive(Clone, Default)]
pub struct ReadEvalBroker {
    ledger: decision::ReadEvalLedger,
}

impl ReadEvalBroker {
    /// Creates a broker with an empty decision ledger.
    pub fn new() -> Self {
        Self::default()
    }

    /// Admits one explicit read-eval request or fails closed.
    pub fn admit(&self, cx: &mut Cx, request: ReadEvalRequest) -> Result<Value> {
        if let Err(err) = request.read_policy.require(&read_eval_capability()) {
            let outcome = match err {
                Error::TrustDenied { .. } => ReadEvalOutcome::TrustDenied,
                _ => ReadEvalOutcome::CapDenied,
            };
            self.record(cx, &request, &CapabilitySet::new(), outcome)?;
            return Err(err);
        }
        if let Err(err) = cx.require_all(&request.requires) {
            self.record(
                cx,
                &request,
                &CapabilitySet::new(),
                ReadEvalOutcome::MissingPower,
            )?;
            return Err(err);
        }

        let active = diminish_capabilities(cx.capabilities(), &request.allow);
        let value = cx.with_capabilities(active.clone(), |cx| {
            let expr = decode_source(
                cx,
                &request.codec,
                request.source.clone(),
                request.read_policy.clone(),
            )?;
            cx.eval_expr(expr)
        })?;

        let matched = request.expected_shape.check_value(cx, value.clone())?;
        if matched.accepted {
            self.record(cx, &request, &active, ReadEvalOutcome::Admitted)?;
            return Ok(value);
        }

        self.record(cx, &request, &active, ReadEvalOutcome::ShapeDenied)?;
        Err(Error::WrongShape {
            expected: request.expected_shape.id().unwrap_or(ShapeId(0)),
            diagnostics: shape_diagnostics(
                cx,
                request.expected_shape.as_ref(),
                matched.diagnostics,
            )?,
        })
    }

    /// Returns read-eval decisions recorded in the broker's default run.
    pub fn decisions(&self, cx: &Cx) -> Result<Vec<ReadEvalDecision>> {
        self.ledger.decisions(cx)
    }

    /// Returns read-eval decisions recorded for `run`.
    pub fn decisions_for_run(&self, cx: &Cx, run: &Ref) -> Result<Vec<ReadEvalDecision>> {
        self.ledger.decisions_for_run(cx, run)
    }

    /// Returns raw ledger events recorded for `run`.
    pub fn events_for_run(&self, run: &Ref) -> Result<Vec<Event>> {
        self.ledger.events_for_run(run)
    }

    fn record(
        &self,
        cx: &mut Cx,
        request: &ReadEvalRequest,
        active: &CapabilitySet,
        outcome: ReadEvalOutcome,
    ) -> Result<Event> {
        let decision = decision::decision_from_request(request, active, outcome);
        self.ledger.record(cx, &decision)
    }
}

impl Object for ReadEvalBroker {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<read-eval-broker>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl sim_kernel::ObjectCompat for ReadEvalBroker {
    fn class(&self, cx: &mut Cx) -> Result<sim_kernel::ClassRef> {
        cx.factory().class_stub(
            sim_kernel::ClassId(0),
            Symbol::qualified("read-eval", "Broker"),
        )
    }
}

/// Returns the broker value symbol exported by [`ReadEvalBrokerLib`].
pub fn read_eval_broker_symbol() -> Symbol {
    Symbol::qualified("read-eval", "broker")
}

/// Returns the manifest id for the read-eval broker library.
pub fn read_eval_broker_lib_id() -> Symbol {
    Symbol::qualified("sim", "read-eval-broker")
}

/// Loadable library that registers the read-eval broker value.
pub struct ReadEvalBrokerLib;

impl Lib for ReadEvalBrokerLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: read_eval_broker_lib_id(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Value {
                symbol: read_eval_broker_symbol(),
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.value(
            read_eval_broker_symbol(),
            cx.factory().opaque(Arc::new(ReadEvalBroker::new()))?,
        )?;
        Ok(())
    }
}

/// Installs the read-eval broker library if it is not already loaded.
pub fn install_read_eval_broker(cx: &mut Cx) -> Result<bool> {
    crate::install_once(cx, &ReadEvalBrokerLib)
}

fn decode_source(
    cx: &mut Cx,
    codec: &Symbol,
    source: ReadEvalSource,
    read_policy: ReadPolicy,
) -> Result<Expr> {
    match source {
        ReadEvalSource::Text(text) => decode_with_codec(cx, codec, Input::Text(text), read_policy),
        ReadEvalSource::Bytes(bytes) => {
            decode_with_codec(cx, codec, Input::Bytes(bytes), read_policy)
        }
        ReadEvalSource::Expr(expr) => Ok(expr),
    }
}

fn diminish_capabilities(current: &CapabilitySet, allowed: &CapabilitySet) -> CapabilitySet {
    current
        .iter()
        .filter(|capability| allowed.contains(capability))
        .cloned()
        .fold(CapabilitySet::new(), CapabilitySet::grant)
}

fn shape_diagnostics(
    cx: &mut Cx,
    shape: &dyn Shape,
    diagnostics: Vec<Diagnostic>,
) -> Result<Vec<Diagnostic>> {
    if !diagnostics.is_empty() {
        return Ok(diagnostics);
    }
    let expected = match shape.symbol() {
        Some(symbol) => symbol.to_string(),
        None => shape.describe(cx)?.name,
    };
    Ok(vec![expected_shape_diagnostic(
        expected,
        "read-eval result",
    )])
}

#[cfg(test)]
mod config_tests;

#[cfg(test)]
mod ledger_tests;

#[cfg(test)]
mod tests;
