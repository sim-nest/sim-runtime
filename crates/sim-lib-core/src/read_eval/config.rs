//! Config data-node adapter for explicit read-eval.

use std::{collections::BTreeMap, sync::Arc};

use sim_kernel::{
    CapabilityName, CapabilitySet, Cx, Error, Expr, ReadPolicy, Result, Shape, ShapeId, Symbol,
    TrustLevel, read_eval_capability,
};
use sim_shape::{expected_shape_diagnostic, parse_shape_expr};

use super::{ReadEvalBroker, ReadEvalRequest, ReadEvalSource, RequestOrigin};

/// Host-owned opt-in for realizing explicit config eval nodes.
///
/// The policy is constructed by the host, not by config text. A config node can
/// request a diminished capability set for the eval body, but it cannot create
/// trust or grant capabilities that the caller does not already hold.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostConfigEvalOptIn {
    read_policy: ReadPolicy,
}

impl HostConfigEvalOptIn {
    /// Creates an opt-in from a host-built read policy.
    pub fn new(read_policy: ReadPolicy) -> Self {
        Self { read_policy }
    }

    /// Creates a trusted-source opt-in and grants the read-eval capability to
    /// the read policy.
    pub fn trusted(capabilities: CapabilitySet) -> Self {
        Self::new(ReadPolicy {
            trust: TrustLevel::TrustedSource,
            capabilities: capabilities.grant(read_eval_capability()),
        })
    }

    /// Returns the host-owned read policy used for broker admission.
    pub fn read_policy(&self) -> &ReadPolicy {
        &self.read_policy
    }
}

/// Parsed data carried by an explicit `config/eval` node.
#[derive(Clone)]
pub struct ConfigEvalNode {
    /// Codec used when the source is text or bytes.
    pub codec: Symbol,
    /// Source expression, text, or bytes admitted through the broker.
    pub source: ReadEvalSource,
    /// Capabilities the caller must already hold before eval can run.
    pub requires: Vec<CapabilityName>,
    /// Maximum capabilities the eval body may run with.
    pub allow: CapabilitySet,
    /// Shape the evaluated result must match before it is merged.
    pub expected_shape: Arc<dyn Shape>,
}

impl ConfigEvalNode {
    fn into_request(self, read_policy: ReadPolicy, detail: Expr) -> ReadEvalRequest {
        ReadEvalRequest {
            origin: RequestOrigin::with_detail(config_eval_origin_tag(), detail),
            codec: self.codec,
            source: self.source,
            read_policy,
            requires: self.requires,
            allow: self.allow,
            expected_shape: self.expected_shape,
        }
    }
}

/// Returns the explicit config eval node symbol.
pub fn config_eval_node_symbol() -> Symbol {
    Symbol::qualified("config", "eval")
}

/// Returns the origin tag used for config eval broker ledger entries.
pub fn config_eval_origin_tag() -> Symbol {
    Symbol::qualified("config", "node")
}

/// Parses an explicit config eval node from expression data.
///
/// The adapter accepts two inert data shapes: the field map stored under a
/// config table's `config/eval` entry, and a list headed by the `config/eval`
/// symbol with field pairs. The required fields are `codec`, exactly one of
/// `source` or `expr`, `requires`, `allow`, and `shape`.
pub fn parse_config_eval_node(expr: &Expr) -> Result<ConfigEvalNode> {
    let fields = match expr {
        Expr::Map(entries) => collect_map_fields(entries)?,
        Expr::List(items) => collect_list_fields(items)?,
        _ => {
            return Err(config_eval_error(
                "config/eval node must be a field map or headed list",
            ));
        }
    };
    build_node(fields)
}

/// Realizes explicit config eval nodes in a decoded config expression.
///
/// With no opt-in, the expression is returned unchanged and no node is parsed or
/// evaluated. With opt-in, `config/eval` entries in maps are admitted through
/// the broker and their map result is merged into that map. A list headed by
/// `config/eval` in value position is replaced by its admitted result.
pub fn realize_config_expr(
    cx: &mut Cx,
    broker: &ReadEvalBroker,
    opt_in: Option<&HostConfigEvalOptIn>,
    expr: Expr,
) -> Result<Expr> {
    let Some(opt_in) = opt_in else {
        return Ok(expr);
    };
    realize_expr_inner(cx, broker, opt_in, expr)
}

fn realize_expr_inner(
    cx: &mut Cx,
    broker: &ReadEvalBroker,
    opt_in: &HostConfigEvalOptIn,
    expr: Expr,
) -> Result<Expr> {
    match expr {
        Expr::Map(entries) => realize_map(cx, broker, opt_in, entries),
        Expr::List(items) if list_head_is_config_eval(&items) => {
            admit_node_expr(cx, broker, opt_in, Expr::List(items))
        }
        Expr::List(items) => items
            .into_iter()
            .map(|item| realize_expr_inner(cx, broker, opt_in, item))
            .collect::<Result<Vec<_>>>()
            .map(Expr::List),
        Expr::Vector(items) => items
            .into_iter()
            .map(|item| realize_expr_inner(cx, broker, opt_in, item))
            .collect::<Result<Vec<_>>>()
            .map(Expr::Vector),
        Expr::Set(items) => items
            .into_iter()
            .map(|item| realize_expr_inner(cx, broker, opt_in, item))
            .collect::<Result<Vec<_>>>()
            .map(Expr::Set),
        other => Ok(other),
    }
}

fn realize_map(
    cx: &mut Cx,
    broker: &ReadEvalBroker,
    opt_in: &HostConfigEvalOptIn,
    entries: Vec<(Expr, Expr)>,
) -> Result<Expr> {
    let mut realized = Vec::<(Expr, Expr)>::new();
    for (key, value) in entries {
        if key_is_config_eval(&key) {
            let merged = admit_node_expr(cx, broker, opt_in, value)?;
            let Expr::Map(entries) = merged else {
                return Err(config_eval_error(
                    "config/eval map entry must produce a map result",
                ));
            };
            for entry in entries {
                push_unique_entry(&mut realized, entry)?;
            }
        } else {
            let value = realize_expr_inner(cx, broker, opt_in, value)?;
            push_unique_entry(&mut realized, (key, value))?;
        }
    }
    Ok(Expr::Map(realized))
}

fn admit_node_expr(
    cx: &mut Cx,
    broker: &ReadEvalBroker,
    opt_in: &HostConfigEvalOptIn,
    node_expr: Expr,
) -> Result<Expr> {
    let node = parse_config_eval_node(&node_expr)?;
    let value = broker.admit(
        cx,
        node.into_request(opt_in.read_policy().clone(), node_expr),
    )?;
    value.object().as_expr(cx)
}

fn push_unique_entry(target: &mut Vec<(Expr, Expr)>, entry: (Expr, Expr)) -> Result<()> {
    if target.iter().any(|(key, _)| key == &entry.0) {
        return Err(config_eval_error(
            "config/eval merge produced a duplicate key",
        ));
    }
    target.push(entry);
    Ok(())
}

fn build_node(fields: BTreeMap<String, &Expr>) -> Result<ConfigEvalNode> {
    let codec = parse_symbol(required_field(&fields, "codec")?, "codec")?;
    let requires = parse_capability_list(required_field(&fields, "requires")?, "requires")?;
    let allow = parse_capability_set(required_field(&fields, "allow")?, "allow")?;
    let expected_shape = parse_shape_field(required_field(&fields, "shape")?)?;
    let source = match (fields.get("source"), fields.get("expr")) {
        (Some(_), Some(_)) => {
            return Err(config_eval_error(
                "config/eval node must carry only one of source or expr",
            ));
        }
        (Some(source), None) => parse_source_field(source)?,
        (None, Some(expr)) => ReadEvalSource::Expr((*expr).clone()),
        (None, None) => {
            return Err(config_eval_error(
                "config/eval node requires source or expr",
            ));
        }
    };

    Ok(ConfigEvalNode {
        codec,
        source,
        requires,
        allow,
        expected_shape,
    })
}

fn collect_map_fields(entries: &[(Expr, Expr)]) -> Result<BTreeMap<String, &Expr>> {
    let mut fields = BTreeMap::new();
    for (key, value) in entries {
        let name = field_name(key)?;
        if fields.insert(name.clone(), value).is_some() {
            return Err(config_eval_error(format!(
                "config/eval node repeats field {name:?}"
            )));
        }
    }
    Ok(fields)
}

fn collect_list_fields(items: &[Expr]) -> Result<BTreeMap<String, &Expr>> {
    let Some((head, tail)) = items.split_first() else {
        return Err(config_eval_error("config/eval list cannot be empty"));
    };
    if !expr_is_config_eval(head) {
        return Err(config_eval_error("config/eval list has the wrong head"));
    }
    let tail = if matches!(tail.first(), Some(version) if expr_text(version) == Some("v1")) {
        &tail[1..]
    } else {
        tail
    };
    if tail.len() % 2 != 0 {
        return Err(config_eval_error(
            "config/eval list fields must be key/value pairs",
        ));
    }
    let mut fields = BTreeMap::new();
    for pair in tail.chunks_exact(2) {
        let name = field_name(&pair[0])?;
        if fields.insert(name.clone(), &pair[1]).is_some() {
            return Err(config_eval_error(format!(
                "config/eval node repeats field {name:?}"
            )));
        }
    }
    Ok(fields)
}

fn required_field<'a>(fields: &'a BTreeMap<String, &Expr>, name: &str) -> Result<&'a Expr> {
    fields
        .get(name)
        .copied()
        .ok_or_else(|| config_eval_error(format!("config/eval node requires {name:?}")))
}

fn parse_source_field(expr: &Expr) -> Result<ReadEvalSource> {
    match expr {
        Expr::String(text) => Ok(ReadEvalSource::Text(text.clone())),
        Expr::Bytes(bytes) => Ok(ReadEvalSource::Bytes(bytes.clone())),
        _ => Err(config_eval_error("source must be a string or bytes value")),
    }
}

fn parse_shape_field(expr: &Expr) -> Result<Arc<dyn Shape>> {
    match expr {
        Expr::String(text) => parse_shape_expr(&Expr::Symbol(parse_symbol_text(text))),
        Expr::Symbol(_) | Expr::List(_) => parse_shape_expr(expr),
        _ => Err(Error::WrongShape {
            expected: ShapeId(0),
            diagnostics: vec![expected_shape_diagnostic(
                "shape expression",
                "config/eval shape",
            )],
        }),
    }
}

fn parse_capability_list(expr: &Expr, field: &str) -> Result<Vec<CapabilityName>> {
    capability_items(expr, field)?
        .iter()
        .map(parse_capability)
        .collect()
}

fn parse_capability_set(expr: &Expr, field: &str) -> Result<CapabilitySet> {
    parse_capability_list(expr, field).map(|capabilities| {
        capabilities
            .into_iter()
            .fold(CapabilitySet::new(), CapabilitySet::grant)
    })
}

fn capability_items<'a>(expr: &'a Expr, field: &str) -> Result<&'a [Expr]> {
    match expr {
        Expr::List(items) | Expr::Vector(items) | Expr::Set(items) => Ok(items),
        _ => Err(config_eval_error(format!(
            "{field} must be a list of capability names"
        ))),
    }
}

fn parse_capability(expr: &Expr) -> Result<CapabilityName> {
    let text = match expr {
        Expr::String(text) => text.clone(),
        Expr::Symbol(symbol) => symbol_text(symbol),
        _ => {
            return Err(config_eval_error(
                "capability entries must be strings or symbols",
            ));
        }
    };
    let text = strip_field_prefix(&text);
    let text = text.strip_prefix("capability/").unwrap_or(text).to_owned();
    Ok(CapabilityName::new(text))
}

fn parse_symbol(expr: &Expr, field: &str) -> Result<Symbol> {
    match expr {
        Expr::String(text) => Ok(parse_symbol_text(text)),
        Expr::Symbol(symbol) => Ok(symbol.clone()),
        _ => Err(config_eval_error(format!(
            "{field} must be a string or symbol"
        ))),
    }
}

fn parse_symbol_text(text: &str) -> Symbol {
    let text = strip_field_prefix(text);
    match text.split_once('/') {
        Some((namespace, name)) if !namespace.is_empty() && !name.is_empty() => {
            Symbol::qualified(namespace.to_owned(), name.to_owned())
        }
        _ => Symbol::new(text.to_owned()),
    }
}

fn field_name(expr: &Expr) -> Result<String> {
    match expr {
        Expr::String(text) => Ok(strip_field_prefix(text).to_owned()),
        Expr::Symbol(symbol) => Ok(strip_field_prefix(&symbol_text(symbol)).to_owned()),
        _ => Err(config_eval_error(
            "config/eval field names must be strings or symbols",
        )),
    }
}

fn list_head_is_config_eval(items: &[Expr]) -> bool {
    items.first().is_some_and(expr_is_config_eval)
}

fn key_is_config_eval(expr: &Expr) -> bool {
    expr_is_config_eval(expr)
}

fn expr_is_config_eval(expr: &Expr) -> bool {
    match expr {
        Expr::Symbol(symbol) => symbol_is_config_eval(symbol),
        Expr::String(text) => text == "config/eval",
        _ => false,
    }
}

fn symbol_is_config_eval(symbol: &Symbol) -> bool {
    symbol == &config_eval_node_symbol()
        || (symbol.namespace.is_none() && symbol.name.as_ref() == "config/eval")
}

fn expr_text(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::String(text) => Some(text),
        Expr::Symbol(symbol) if symbol.namespace.is_none() => Some(symbol.name.as_ref()),
        _ => None,
    }
}

fn symbol_text(symbol: &Symbol) -> String {
    match &symbol.namespace {
        Some(namespace) => format!("{namespace}/{}", symbol.name),
        None => symbol.name.to_string(),
    }
}

fn strip_field_prefix(text: &str) -> &str {
    text.strip_prefix(':').unwrap_or(text)
}

fn config_eval_error(message: impl Into<String>) -> Error {
    Error::domain_error(
        config_eval_node_symbol(),
        Symbol::qualified("config", "eval-node"),
        message,
    )
}
