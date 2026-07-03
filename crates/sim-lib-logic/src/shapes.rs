use std::sync::Arc;

use sim_kernel::{Cx, Error, Expr, Result, Shape, ShapeDoc, ShapeMatch, Symbol, Value};
use sim_shape::{AnyShape, shape_value};

use crate::clause::{is_goal_expr, parse_clause_expr};

pub fn register_logic_shapes(
    linker: &mut sim_kernel::Linker<'_>,
    cx: &mut sim_kernel::LoadCx,
) -> Result<()> {
    let _ = cx;
    for (symbol, shape) in [
        (
            Symbol::qualified("logic", "Var"),
            Arc::new(VarShape) as Arc<dyn Shape>,
        ),
        (
            Symbol::qualified("logic", "Goal"),
            Arc::new(GoalShape) as Arc<dyn Shape>,
        ),
        (
            Symbol::qualified("logic", "Clause"),
            Arc::new(ClauseShape(None)) as Arc<dyn Shape>,
        ),
        (
            Symbol::qualified("logic", "Fact"),
            Arc::new(ClauseShape(Some("fact"))) as Arc<dyn Shape>,
        ),
        (
            Symbol::qualified("logic", "Rule"),
            Arc::new(ClauseShape(Some("rule"))) as Arc<dyn Shape>,
        ),
        (
            Symbol::qualified("logic", "Answer"),
            Arc::new(AnswerShape) as Arc<dyn Shape>,
        ),
    ] {
        linker.shape_value(symbol.clone(), shape_value(symbol, shape))?;
    }
    linker.shape_value(
        Symbol::qualified("logic", "Config"),
        shape_value(Symbol::qualified("logic", "Config"), Arc::new(AnyShape)),
    )?;
    Ok(())
}

struct VarShape;
struct GoalShape;
struct ClauseShape(Option<&'static str>);
struct AnswerShape;

impl Shape for VarShape {
    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let expr = value.object().as_expr(cx)?;
        self.check_expr(cx, &expr)
    }

    fn check_expr(&self, _cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        Ok(if matches!(expr, Expr::Local(_)) {
            ShapeMatch::accept(sim_kernel::MatchScore::exact(100))
        } else {
            ShapeMatch::reject("logic variable expected")
        })
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("LogicVar").with_detail("?x or Expr::Local"))
    }
}

impl Shape for GoalShape {
    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let expr = value.object().as_expr(cx)?;
        self.check_expr(cx, &expr)
    }

    fn check_expr(&self, _cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        Ok(if is_goal_expr(expr) {
            ShapeMatch::accept(sim_kernel::MatchScore::exact(100))
        } else {
            ShapeMatch::reject("logic goal expected")
        })
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("LogicGoal").with_detail("call-shaped goal expression"))
    }
}

impl Shape for ClauseShape {
    fn check_value(&self, cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        let expr = value.object().as_expr(cx)?;
        self.check_expr(cx, &expr)
    }

    fn check_expr(&self, _cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let clause = parse_clause_expr(crate::ClauseId(0), expr.clone());
        let accepted = match (&self.0, clause) {
            (None, Ok(_)) => true,
            (Some("fact"), Ok(clause)) => clause.body.is_empty(),
            (Some("rule"), Ok(clause)) => !clause.body.is_empty(),
            _ => false,
        };
        Ok(if accepted {
            ShapeMatch::accept(sim_kernel::MatchScore::exact(100))
        } else {
            ShapeMatch::reject("logic clause expected")
        })
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        let name = match self.0 {
            Some("fact") => "LogicFact",
            Some("rule") => "LogicRule",
            _ => "LogicClause",
        };
        Ok(ShapeDoc::new(name))
    }
}

impl Shape for AnswerShape {
    fn check_value(&self, _cx: &mut Cx, value: Value) -> Result<ShapeMatch> {
        Ok(
            if value
                .object()
                .downcast_ref::<sim_kernel::ShapeMatchObject>()
                .is_some()
            {
                ShapeMatch::accept(sim_kernel::MatchScore::exact(100))
            } else {
                ShapeMatch::reject("logic answer expected")
            },
        )
    }

    fn check_expr(&self, cx: &mut Cx, expr: &Expr) -> Result<ShapeMatch> {
        let value = cx.eval_expr(expr.clone())?;
        self.check_value(cx, value)
    }

    fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
        Ok(ShapeDoc::new("LogicAnswer").with_detail("ShapeMatch answer"))
    }
}

pub(crate) fn require_logic_stream(value: &Value) -> Result<&crate::stream::LogicStream> {
    value
        .object()
        .downcast_ref::<crate::stream::LogicStream>()
        .ok_or(Error::TypeMismatch {
            expected: "logic stream",
            found: "non-stream",
        })
}
