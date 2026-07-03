use sim_kernel::{Expr, Result, Symbol};
use sim_lib_control::ControlTag;

use crate::error::logic_eval_error;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct NafDemand {
    goal: Expr,
}

impl NafDemand {
    pub(crate) fn new(goal: Expr) -> Self {
        Self { goal }
    }

    pub(crate) fn tag(&self) -> ControlTag {
        ControlTag::new(Self::tag_symbol())
    }

    pub(crate) fn goal(&self) -> &Expr {
        &self.goal
    }

    fn tag_symbol() -> Symbol {
        Symbol::qualified("logic", "naf")
    }
}

pub(crate) fn naf_inner_goal(goal: &Expr) -> Result<Option<&Expr>> {
    let Expr::List(items) = goal else {
        return Ok(None);
    };
    let Some(Expr::Symbol(head)) = items.first() else {
        return Ok(None);
    };
    if head.namespace.is_some() || head.name.as_ref() != "not" {
        return Ok(None);
    }
    let [_, inner] = items.as_slice() else {
        return Err(logic_eval_error("not expects exactly one goal"));
    };
    Ok(Some(inner))
}
