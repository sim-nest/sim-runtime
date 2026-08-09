use std::sync::Arc;

use sim_kernel::{Cx, Expr, ExprKind, Result, ShapeRef, Symbol, Value};
use sim_shape::{
    AnyShape, ExactExprShape, ExprKindShape, ListShape, OneOfShape, Shape, shape_value,
};

/// Source provenance retained after an annotation is erased from evaluation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnotationProvenance {
    /// Exact source spelling.
    pub source: String,
    /// Byte range in the TypeScript source.
    pub span: std::ops::Range<usize>,
    /// Parser contexts copied from the codec's annotation reference.
    pub context: Vec<String>,
    /// Stable origin-chain labels, outermost first.
    pub origins: Vec<String>,
}

/// A faithful, deliberately bounded Shape projection category.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectedShape {
    /// `unknown` or `any`, observationally unconstrained.
    Any,
    /// A primitive expression category.
    Primitive(ExprKind),
    /// An exact literal.
    Literal(Expr),
    /// A set-theoretic union of faithful members.
    Union(Vec<ProjectedShape>),
    /// A fixed tuple of faithful members.
    Tuple(Vec<ProjectedShape>),
    /// A homogeneous array.
    Array(Box<ProjectedShape>),
}

impl ProjectedShape {
    /// Materialize this observational projection as a Shape runtime value.
    pub fn shape_ref(&self, name: Symbol) -> ShapeRef {
        shape_value(name, self.shape())
    }

    fn shape(&self) -> Arc<dyn Shape> {
        match self {
            Self::Any => Arc::new(AnyShape),
            Self::Primitive(kind) => Arc::new(ExprKindShape::new(kind.clone())),
            Self::Literal(value) => Arc::new(ExactExprShape::new(value.clone())),
            Self::Union(members) => Arc::new(OneOfShape::new(
                members.iter().map(ProjectedShape::shape).collect(),
            )),
            Self::Tuple(members) => Arc::new(ListShape::new(
                members.iter().map(ProjectedShape::shape).collect(),
            )),
            Self::Array(member) => Arc::new(ListShape::new(vec![member.shape()])),
        }
    }
}

/// Retained annotation plus an optional faithful browse projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnnotationMetadata {
    /// Erased source provenance.
    pub provenance: AnnotationProvenance,
    /// Projection, absent when the category is unsupported or checker-dependent.
    pub projected: Option<ProjectedShape>,
}

/// Attach admitted argument/result metadata through Shape's neutral adapter.
///
/// The adapter delegates calls unchanged; this function never invokes Shape
/// matching and therefore cannot act as a dynamic guard or source preflight.
pub fn attach_browse_signature(
    cx: &mut Cx,
    callable: Value,
    args: Option<ProjectedShape>,
    result: Option<ProjectedShape>,
) -> Result<Value> {
    let args = args.map(|shape| shape.shape_ref(Symbol::qualified("typescript", "arguments")));
    let result = result.map(|shape| shape.shape_ref(Symbol::qualified("typescript", "result")));
    sim_shape::browse_signature(cx, callable, args, result)
}

/// Project a bounded annotation spelling without binding, inference, or checking.
pub fn project_annotation(source: &str) -> Option<ProjectedShape> {
    let source = source.trim();
    if let Some(parts) = split_top_level(source, '|') {
        return parts
            .into_iter()
            .map(project_annotation)
            .collect::<Option<Vec<_>>>()
            .map(ProjectedShape::Union);
    }
    if source.starts_with('[') && source.ends_with(']') {
        let body = &source[1..source.len() - 1];
        let parts = split_top_level(body, ',').unwrap_or_else(|| vec![body]);
        return parts
            .into_iter()
            .map(project_annotation)
            .collect::<Option<Vec<_>>>()
            .map(ProjectedShape::Tuple);
    }
    if let Some(inner) = source.strip_suffix("[]") {
        return project_annotation(inner).map(|shape| ProjectedShape::Array(Box::new(shape)));
    }
    match source {
        "any" | "unknown" => Some(ProjectedShape::Any),
        "boolean" => Some(ProjectedShape::Primitive(ExprKind::Bool)),
        "string" => Some(ProjectedShape::Primitive(ExprKind::String)),
        "number" | "bigint" => Some(ProjectedShape::Primitive(ExprKind::Number)),
        "null" => Some(ProjectedShape::Literal(Expr::Nil)),
        "true" => Some(ProjectedShape::Literal(Expr::Bool(true))),
        "false" => Some(ProjectedShape::Literal(Expr::Bool(false))),
        _ if source.starts_with('"') && source.ends_with('"') => Some(ProjectedShape::Literal(
            Expr::String(source[1..source.len() - 1].to_owned()),
        )),
        _ => None,
    }
}

fn split_top_level(source: &str, separator: char) -> Option<Vec<&str>> {
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut parts = Vec::new();
    for (index, ch) in source.char_indices() {
        match ch {
            '[' | '(' | '{' | '<' => depth += 1,
            ']' | ')' | '}' | '>' => depth = depth.saturating_sub(1),
            ch if ch == separator && depth == 0 => {
                parts.push(source[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    if parts.is_empty() {
        None
    } else {
        parts.push(source[start..].trim());
        Some(parts)
    }
}
