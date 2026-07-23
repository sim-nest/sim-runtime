//! Browseable shape contracts for incremental organ callables.

use std::sync::Arc;

use sim_kernel::{
    Cx, Expr, ExprKind, Result, Shape, ShapeDoc, ShapeMatch, Symbol, Value, shape::MatchScore,
};
use sim_shape::{AnyShape, ExprKindShape, ListShape, shape_value};

use crate::IncrementalSession;

/// Shape symbol for incremental engine session objects.
pub fn incremental_engine_shape_symbol() -> Symbol {
    Symbol::qualified("incremental", "Engine")
}

/// Shape symbol for query keys.
pub fn incremental_key_shape_symbol() -> Symbol {
    Symbol::qualified("incremental", "Key")
}

/// Shape symbol for query source expressions.
pub fn incremental_query_expr_shape_symbol() -> Symbol {
    Symbol::qualified("incremental", "QueryExpr")
}

/// Shape symbol for projected reports.
pub fn incremental_report_shape_symbol() -> Symbol {
    Symbol::qualified("incremental", "Report")
}

pub(crate) fn register_incremental_shapes(
    linker: &mut sim_kernel::Linker<'_>,
    _cx: &mut sim_kernel::LoadCx,
) -> Result<()> {
    for (symbol, shape) in [
        (
            incremental_engine_shape_symbol(),
            Arc::new(EngineShape) as Arc<dyn Shape>,
        ),
        (
            incremental_key_shape_symbol(),
            Arc::new(ExprKindShape::new(ExprKind::String)) as Arc<dyn Shape>,
        ),
        (
            incremental_query_expr_shape_symbol(),
            Arc::new(AnyShape) as Arc<dyn Shape>,
        ),
        (
            incremental_report_shape_symbol(),
            Arc::new(AnyShape) as Arc<dyn Shape>,
        ),
    ] {
        linker.shape_value(symbol.clone(), shape_value(symbol, shape))?;
    }
    Ok(())
}

pub(crate) fn args_shape(symbol: Symbol, items: Vec<Arc<dyn Shape>>) -> sim_kernel::ShapeRef {
    shape_value(symbol, Arc::new(ListShape::tuple(items)))
}

pub(crate) fn result_shape(symbol: Symbol) -> sim_kernel::ShapeRef {
    shape_value(symbol, Arc::new(AnyShape))
}

pub(crate) fn engine_shape() -> Arc<dyn Shape> {
    Arc::new(EngineShape)
}

pub(crate) fn key_shape() -> Arc<dyn Shape> {
    Arc::new(ExprKindShape::new(ExprKind::String))
}

pub(crate) fn query_expr_shape() -> Arc<dyn Shape> {
    Arc::new(AnyShape)
}

pub(crate) fn report_shape() -> Arc<dyn Shape> {
    Arc::new(AnyShape)
}

struct EngineShape;

impl Shape for EngineShape {
    fn symbol(&self) -> Option<Symbol> {
        Some(incremental_engine_shape_symbol())
    }

    fn check_value(&self, _cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        Ok(
            if value
                .object()
                .downcast_ref::<IncrementalSession>()
                .is_some()
            {
                ShapeMatch::accept(MatchScore::exact(100))
            } else {
                ShapeMatch::reject("incremental engine expected")
            },
        )
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let value = cx.eval_expr(expr.clone())?;
        self.check_value(cx, value)
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("IncrementalEngine"))
    }
}
