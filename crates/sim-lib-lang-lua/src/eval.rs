use std::sync::Arc;

use sim_kernel::{Args, Cx, Error, Expr, Result, Symbol, Value};
use sim_lib_standard_core::{
    Arity, CoercionPolicy, GuestRuntimeKit, SharedOrganRuntime, TruthPolicy,
};

use crate::{LuaEnv, LuaResult, lua_core_profile};

/// Eval policy for the Lua core profile.
#[derive(Clone, Debug)]
pub struct LuaEvalPolicy {
    kit: GuestRuntimeKit,
}

impl LuaEvalPolicy {
    /// Build the Lua core eval policy and register its runtime kit.
    pub fn new(cx: &mut Cx) -> Result<Self> {
        let mut runtime = SharedOrganRuntime::new();
        let profile = lua_core_profile();
        let profile_symbol = profile.symbol.clone();
        runtime.register_profile(profile)?;
        runtime.register_kit(&profile_symbol, lua_runtime_kit(cx)?)?;
        let kit = runtime
            .kit(&profile_symbol)
            .cloned()
            .ok_or(Error::UnknownSymbol {
                symbol: profile_symbol,
            })?;
        Ok(Self { kit })
    }

    /// Borrow the language-neutral runtime policy kit.
    pub fn kit(&self) -> &GuestRuntimeKit {
        &self.kit
    }

    /// Evaluate a Lua core expression.
    pub fn eval(&self, cx: &mut Cx, env: &mut LuaEnv, expr: &Expr) -> Result<LuaResult> {
        if let Some((form, args)) = lua_form(expr) {
            return match form {
                LuaForm::Chunk | LuaForm::Block => self.eval_block(cx, env, args),
                LuaForm::Local => self.eval_local(cx, env, args),
                LuaForm::Assign => self.eval_assign(cx, env, args),
                LuaForm::If => self.eval_if(cx, env, args),
                LuaForm::Call => self.eval_call_form(cx, env, args),
                LuaForm::Return => self.eval_return(cx, env, args),
            };
        }

        match expr {
            Expr::Block(body) => self.eval_block(cx, env, body),
            Expr::Call { operator, args } => self.eval_call(cx, env, operator, args),
            _ => self.eval_atom(cx, env, expr).map(LuaResult::one),
        }
    }

    fn eval_block(&self, cx: &mut Cx, env: &mut LuaEnv, body: &[Expr]) -> Result<LuaResult> {
        let mut last = vec![self.kit.nil.clone()];
        for expr in body {
            let result = self.eval(cx, env, expr)?;
            if result.is_return() {
                return Ok(result);
            }
            last = self
                .kit
                .adjust_values(result.into_values(), Arity::AtLeastOne);
        }
        Ok(LuaResult::values(last))
    }

    fn eval_local(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        let (name_expr, value_exprs) = required_head(args, "lua local")?;
        if value_exprs.len() > 1 {
            return Err(Error::Eval("lua local accepts one initializer".to_owned()));
        }
        let name = binding_symbol(name_expr, "lua local")?;
        let value = match value_exprs.first() {
            Some(value_expr) => self.eval_one(cx, env, value_expr)?,
            None => self.kit.nil.clone(),
        };
        env.define(name, value.clone());
        Ok(LuaResult::one(value))
    }

    fn eval_assign(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        let (name_expr, value_exprs) = required_head(args, "lua assign")?;
        if value_exprs.len() != 1 {
            return Err(Error::Eval("lua assign requires one value".to_owned()));
        }
        let name = binding_symbol(name_expr, "lua assign")?;
        let value = self.eval_one(cx, env, &value_exprs[0])?;
        let assigned = env.assign(&name, value)?;
        Ok(LuaResult::one(assigned))
    }

    fn eval_if(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        if !(2..=3).contains(&args.len()) {
            return Err(Error::Eval(
                "lua if requires condition, then, and optional else".to_owned(),
            ));
        }
        let condition = self.eval_one(cx, env, &args[0])?;
        if self.kit.is_truthy(cx, &condition)? {
            self.eval(cx, env, &args[1])
        } else if let Some(else_expr) = args.get(2) {
            self.eval(cx, env, else_expr)
        } else {
            Ok(LuaResult::one(self.kit.nil.clone()))
        }
    }

    fn eval_call_form(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        let (operator, value_exprs) = required_head(args, "lua call")?;
        self.eval_call(cx, env, operator, value_exprs)
    }

    fn eval_call(
        &self,
        cx: &mut Cx,
        env: &mut LuaEnv,
        operator: &Expr,
        args: &[Expr],
    ) -> Result<LuaResult> {
        let callee = self.eval_one(cx, env, operator)?;
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval_one(cx, env, arg)?);
        }
        cx.call_value(callee, Args::new(values)).map(LuaResult::one)
    }

    fn eval_return(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval_one(cx, env, arg)?);
        }
        Ok(LuaResult::return_values(
            self.kit.adjust_values(values, Arity::All),
        ))
    }

    fn eval_one(&self, cx: &mut Cx, env: &mut LuaEnv, expr: &Expr) -> Result<Value> {
        match self.eval(cx, env, expr)? {
            LuaResult::Values(values) => Ok(self
                .kit
                .adjust_values(values, Arity::AtLeastOne)
                .into_iter()
                .next()
                .unwrap_or_else(|| self.kit.nil.clone())),
            LuaResult::Return(_) => Err(Error::Eval(
                "lua return cannot be used as a value expression".to_owned(),
            )),
        }
    }

    fn eval_atom(&self, cx: &mut Cx, env: &mut LuaEnv, expr: &Expr) -> Result<Value> {
        match expr {
            Expr::Nil => Ok(self.kit.nil.clone()),
            Expr::Bool(value) => cx.factory().bool(*value),
            Expr::Number(number) => cx
                .factory()
                .number_literal(number.domain.clone(), number.canonical.clone()),
            Expr::String(value) => cx.factory().string(value.clone()),
            Expr::Bytes(value) => cx.factory().bytes(value.clone()),
            Expr::Symbol(symbol) => {
                if env.contains(symbol) {
                    env.get(symbol)
                } else {
                    cx.factory().symbol(symbol.clone())
                }
            }
            Expr::Local(symbol) => env.get(symbol),
            Expr::List(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(self.eval_one(cx, env, item)?);
                }
                cx.factory().list(values)
            }
            Expr::Vector(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(self.eval_one(cx, env, item)?);
                }
                cx.factory().list(values)
            }
            Expr::Map(entries) => {
                let mut values = Vec::with_capacity(entries.len() * 2);
                for (key, value) in entries {
                    values.push(self.eval_one(cx, env, key)?);
                    values.push(self.eval_one(cx, env, value)?);
                }
                cx.factory().list(values)
            }
            Expr::Set(items) => {
                let mut values = Vec::with_capacity(items.len());
                for item in items {
                    values.push(self.eval_one(cx, env, item)?);
                }
                cx.factory().list(values)
            }
            Expr::Block(body) => match self.eval_block(cx, env, body)? {
                LuaResult::Values(values) => Ok(self
                    .kit
                    .adjust_values(values, Arity::AtLeastOne)
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| self.kit.nil.clone())),
                LuaResult::Return(_) => Err(Error::Eval(
                    "lua return cannot be used as a value expression".to_owned(),
                )),
            },
            Expr::Quote { expr, .. } => cx.factory().expr((**expr).clone()),
            Expr::Annotated { expr, .. } => self.eval_one(cx, env, expr),
            Expr::Extension { .. }
            | Expr::Infix { .. }
            | Expr::Prefix { .. }
            | Expr::Postfix { .. } => cx.factory().expr(expr.clone()),
            Expr::Call { .. } => unreachable!("calls are handled before atom evaluation"),
        }
    }
}

fn lua_runtime_kit(cx: &mut Cx) -> Result<GuestRuntimeKit> {
    Ok(GuestRuntimeKit::new(
        Arc::new(LuaTruthPolicy),
        Arc::new(LuaCoercionPolicy),
        cx.factory().nil()?,
    ))
}

#[derive(Clone, Copy)]
enum LuaForm {
    Chunk,
    Block,
    Local,
    Assign,
    If,
    Call,
    Return,
}

fn lua_form(expr: &Expr) -> Option<(LuaForm, &[Expr])> {
    let Expr::List(items) = expr else {
        return None;
    };
    let (head, args) = items.split_first()?;
    let Expr::Symbol(symbol) = head else {
        return None;
    };
    lua_form_symbol(symbol).map(|form| (form, args))
}

fn lua_form_symbol(symbol: &Symbol) -> Option<LuaForm> {
    if !matches!(
        symbol.namespace.as_deref(),
        Some("lua") | Some("lua/core") | None
    ) {
        return None;
    }
    match symbol.name.as_ref() {
        "chunk" => Some(LuaForm::Chunk),
        "block" => Some(LuaForm::Block),
        "local" => Some(LuaForm::Local),
        "assign" => Some(LuaForm::Assign),
        "if" => Some(LuaForm::If),
        "call" => Some(LuaForm::Call),
        "return" => Some(LuaForm::Return),
        _ => None,
    }
}

fn required_head<'a>(args: &'a [Expr], context: &str) -> Result<(&'a Expr, &'a [Expr])> {
    args.split_first()
        .ok_or_else(|| Error::Eval(format!("{context} requires a target")))
}

fn binding_symbol(expr: &Expr, context: &str) -> Result<Symbol> {
    match expr {
        Expr::Symbol(symbol) | Expr::Local(symbol) => Ok(symbol.clone()),
        _ => Err(Error::Eval(format!(
            "{context} requires a symbol binding target"
        ))),
    }
}

struct LuaTruthPolicy;

impl TruthPolicy for LuaTruthPolicy {
    fn is_truthy(&self, cx: &mut Cx, value: &Value) -> Result<bool> {
        Ok(!matches!(
            value.object().as_expr(cx)?,
            Expr::Nil | Expr::Bool(false)
        ))
    }
}

struct LuaCoercionPolicy;

impl CoercionPolicy for LuaCoercionPolicy {
    fn to_number(&self, _cx: &mut Cx, _value: &Value) -> Result<Option<Value>> {
        Ok(None)
    }

    fn to_string(&self, _cx: &mut Cx, _value: &Value) -> Result<Option<Value>> {
        Ok(None)
    }
}
