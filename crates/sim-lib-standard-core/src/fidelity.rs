//! Fidelity badges recording how faithfully a profile realizes an organ.

use sim_kernel::{ContentId, Coordinate, Expr, HandleId, Ref, Symbol};
pub(crate) use sim_value::kind::expr_kind;

/// A fidelity badge: how faithfully a subject realizes a named badge, at a
/// given level, with a reference to the evidence behind it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FidelityBadge {
    /// What the badge is about (typically a profile symbol ref).
    pub subject: Ref,
    /// The badge name.
    pub badge: Symbol,
    /// Fidelity level, higher meaning more faithful.
    pub level: u8,
    /// Reference to the evidence (typically a conformance test) for the badge.
    pub evidence: Ref,
}

impl FidelityBadge {
    /// Construct a badge from its subject, name, level, and evidence.
    pub fn new(subject: Ref, badge: Symbol, level: u8, evidence: Ref) -> Self {
        Self {
            subject,
            badge,
            level,
            evidence,
        }
    }

    /// Encode this badge as constructor arguments for the `standard/FidelityBadge` class.
    pub fn to_constructor_args(&self) -> Vec<Expr> {
        vec![
            ref_expr(&self.subject),
            Expr::Symbol(self.badge.clone()),
            Expr::String(self.level.to_string()),
            ref_expr(&self.evidence),
        ]
    }

    /// Decode a badge from `standard/FidelityBadge` constructor arguments.
    pub fn from_constructor_args(args: Vec<Expr>) -> sim_kernel::Result<Self> {
        let [subject, badge, level, evidence] = args.as_slice() else {
            return Err(sim_kernel::Error::Eval(
                "standard/FidelityBadge expects subject, badge, level, evidence".to_owned(),
            ));
        };
        Ok(Self {
            subject: ref_from_expr(subject)?,
            badge: symbol_from_expr(badge, "badge")?,
            level: level_from_expr(level)?,
            evidence: ref_from_expr(evidence)?,
        })
    }
}

/// Class symbol for the `standard/FidelityBadge` runtime object.
pub fn fidelity_badge_class_symbol() -> Symbol {
    Symbol::qualified("standard", "FidelityBadge")
}

pub(crate) fn ref_expr(reference: &Ref) -> Expr {
    match reference {
        Ref::Symbol(symbol) => Expr::Symbol(symbol.clone()),
        Ref::Content(content) => Expr::List(vec![
            Expr::Symbol(ref_content_symbol()),
            Expr::Symbol(content.algorithm.clone()),
            Expr::Bytes(content.bytes.to_vec()),
        ]),
        Ref::Handle(handle) => Expr::List(vec![
            Expr::Symbol(ref_handle_symbol()),
            Expr::Bytes(handle.0.to_be_bytes().to_vec()),
        ]),
        Ref::Coord(coordinate) => Expr::List(vec![
            Expr::Symbol(ref_coord_symbol()),
            Expr::Symbol(coordinate.space.clone()),
            ref_expr(&Ref::Content(coordinate.ordinal.clone())),
        ]),
    }
}

pub(crate) fn ref_from_expr(expr: &Expr) -> sim_kernel::Result<Ref> {
    match expr {
        Expr::Symbol(symbol) => Ok(Ref::Symbol(symbol.clone())),
        Expr::List(items) => ref_from_list(items),
        _ => Err(sim_kernel::Error::TypeMismatch {
            expected: "symbol ref",
            found: expr_kind(expr),
        }),
    }
}

pub(crate) fn symbol_from_expr(expr: &Expr, expected: &'static str) -> sim_kernel::Result<Symbol> {
    match expr {
        Expr::Symbol(symbol) => Ok(symbol.clone()),
        _ => Err(sim_kernel::Error::TypeMismatch {
            expected,
            found: expr_kind(expr),
        }),
    }
}

fn level_from_expr(expr: &Expr) -> sim_kernel::Result<u8> {
    let Expr::String(level) = expr else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "level string",
            found: expr_kind(expr),
        });
    };
    level
        .parse::<u8>()
        .map_err(|err| sim_kernel::Error::Eval(format!("invalid fidelity level {level}: {err}")))
}

fn ref_from_list(items: &[Expr]) -> sim_kernel::Result<Ref> {
    let Some(Expr::Symbol(head)) = items.first() else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "ref constructor head",
            found: items.first().map(expr_kind).unwrap_or("empty list"),
        });
    };
    if head == &ref_content_symbol() {
        return content_ref_from_items(items);
    }
    if head == &ref_handle_symbol() {
        return handle_ref_from_items(items);
    }
    if head == &ref_coord_symbol() {
        return coord_ref_from_items(items);
    }
    Err(sim_kernel::Error::Eval(format!(
        "unknown standard ref constructor {head}"
    )))
}

fn content_ref_from_items(items: &[Expr]) -> sim_kernel::Result<Ref> {
    let [_, Expr::Symbol(algorithm), Expr::Bytes(bytes)] = items else {
        return Err(sim_kernel::Error::Eval(
            "standard/ref-content expects algorithm and 32 bytes".to_owned(),
        ));
    };
    let bytes = bytes_32(bytes)?;
    Ok(Ref::Content(ContentId::from_bytes(
        algorithm.clone(),
        bytes,
    )))
}

fn handle_ref_from_items(items: &[Expr]) -> sim_kernel::Result<Ref> {
    let [_, Expr::Bytes(bytes)] = items else {
        return Err(sim_kernel::Error::Eval(
            "standard/ref-handle expects 16 bytes".to_owned(),
        ));
    };
    let bytes: [u8; 16] = bytes.as_slice().try_into().map_err(|_| {
        sim_kernel::Error::Eval(format!(
            "handle ref expects 16 bytes, found {}",
            bytes.len()
        ))
    })?;
    Ok(Ref::Handle(HandleId(u128::from_be_bytes(bytes))))
}

fn coord_ref_from_items(items: &[Expr]) -> sim_kernel::Result<Ref> {
    let [_, Expr::Symbol(space), ordinal] = items else {
        return Err(sim_kernel::Error::Eval(
            "standard/ref-coord expects space and content ordinal".to_owned(),
        ));
    };
    let Ref::Content(ordinal) = ref_from_expr(ordinal)? else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "content ordinal ref",
            found: expr_kind(ordinal),
        });
    };
    Ok(Ref::Coord(Coordinate {
        space: space.clone(),
        ordinal,
    }))
}

fn bytes_32(bytes: &[u8]) -> sim_kernel::Result<[u8; 32]> {
    bytes.try_into().map_err(|_| {
        sim_kernel::Error::Eval(format!(
            "content ref expects 32 bytes, found {}",
            bytes.len()
        ))
    })
}

fn ref_content_symbol() -> Symbol {
    Symbol::qualified("standard", "ref-content")
}

fn ref_handle_symbol() -> Symbol {
    Symbol::qualified("standard", "ref-handle")
}

fn ref_coord_symbol() -> Symbol {
    Symbol::qualified("standard", "ref-coord")
}
