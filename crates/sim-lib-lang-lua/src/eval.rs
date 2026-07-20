use std::sync::Arc;

use sim_kernel::{Cx, Error, Expr, Result, Symbol, Value};
use sim_lib_standard_core::{
    Arity, CoercionPolicy, GuestRuntimeKit, SharedOrganRuntime, TruthPolicy,
};

use crate::{
    LuaEnv, LuaOp, LuaResult,
    call::call_lua_value,
    closure::{lua_closure_value, lua_varargs_values},
    forms::{LuaForm, binding_symbol, bool_literal, lua_form, required_head, symbol_list},
    loops::{eval_generic_for, eval_numeric_for},
    lua_binary, lua_core_profile, lua_get, lua_len, lua_rawget, lua_rawset, lua_table_from_values,
    stdlib_base::install_lua_base_stdlib,
    stdlib_coroutine::install_lua_coroutine_stdlib,
};

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

    /// Install the Lua base and coroutine standard library into `env`.
    pub fn install_stdlib(&self, cx: &mut Cx, env: &mut LuaEnv) -> Result<()> {
        install_lua_base_stdlib(cx, self, env)?;
        install_lua_coroutine_stdlib(cx, self, env)
    }

    /// Evaluate a Lua core expression.
    pub fn eval(&self, cx: &mut Cx, env: &mut LuaEnv, expr: &Expr) -> Result<LuaResult> {
        if let Some((form, args)) = lua_form(expr) {
            return match form {
                LuaForm::Chunk | LuaForm::Block => self.eval_block(cx, env, args),
                LuaForm::Local => self.eval_local(cx, env, args),
                LuaForm::LocalValues => self.eval_local_values(cx, env, args),
                LuaForm::Assign => self.eval_assign(cx, env, args),
                LuaForm::If => self.eval_if(cx, env, args),
                LuaForm::Call => self.eval_call_form(cx, env, args),
                LuaForm::Closure => self.eval_closure(cx, env, args),
                LuaForm::Varargs => self.eval_varargs(env, args),
                LuaForm::Return => self.eval_return(cx, env, args),
                LuaForm::Break => self.eval_break(args),
                LuaForm::NumericFor => {
                    eval_numeric_for(cx, self, env, args, |policy, cx, env, expr| {
                        policy.eval_one(cx, env, expr)
                    })
                }
                LuaForm::GenericFor => {
                    eval_generic_for(cx, self, env, args, |policy, cx, env, expr| {
                        policy.eval_one(cx, env, expr)
                    })
                }
                LuaForm::Stdlib => self.eval_stdlib(cx, env, args),
                LuaForm::Table => self.eval_table(cx, env, args),
                LuaForm::Get => self.eval_get(cx, env, args),
                LuaForm::RawGet => self.eval_rawget(cx, env, args),
                LuaForm::RawSet => self.eval_rawset(cx, env, args),
                LuaForm::Len => self.eval_len(cx, env, args),
                LuaForm::Binary(op) => self.eval_binary(cx, env, op, args),
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
        for (index, expr) in body.iter().enumerate() {
            let result = self.eval(cx, env, expr)?;
            if result.is_return() || result.is_break() {
                return Ok(result);
            }
            let values = result.into_values();
            last = if index + 1 == body.len() {
                values
            } else {
                self.kit.adjust_values(values, Arity::AtLeastOne)
            };
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
        env.define(name, value.clone())?;
        Ok(LuaResult::one(value))
    }

    fn eval_local_values(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        let (names_expr, value_exprs) = required_head(args, "lua local-values")?;
        let names = symbol_list(names_expr, "lua local-values")?;
        let values = self.assignment_values(cx, env, value_exprs, names.len())?;
        for (name, value) in names.into_iter().zip(values.iter().cloned()) {
            env.define(name, value)?;
        }
        Ok(LuaResult::values(values))
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
        let values = self.eval_argument_values(cx, env, args)?;
        call_lua_value(cx, self, callee, values).map(LuaResult::values)
    }

    fn eval_closure(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        if !(4..=5).contains(&args.len()) {
            return Err(Error::Eval(
                "lua closure requires name, params, vararg flag, body, and optional captures"
                    .to_owned(),
            ));
        }
        let name = binding_symbol(&args[0], "lua closure")?;
        let params = symbol_list(&args[1], "lua closure params")?;
        let vararg = bool_literal(&args[2], "lua closure vararg flag")?;
        let captures = match args.get(4) {
            Some(expr) => symbol_list(expr, "lua closure captures")?,
            None => Vec::new(),
        };
        lua_closure_value(cx, env, name, params, vararg, args[3].clone(), captures)
            .map(LuaResult::one)
    }

    fn eval_varargs(&self, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        if !args.is_empty() {
            return Err(Error::Eval("lua varargs accepts no operands".to_owned()));
        }
        let value = env.get(&Symbol::new("..."))?;
        let values = lua_varargs_values(&value)
            .ok_or_else(|| Error::Eval("lua varargs local is not a vararg bundle".to_owned()))?;
        Ok(LuaResult::values(values))
    }

    fn eval_return(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        Ok(LuaResult::return_values(
            self.eval_multi_exprs(cx, env, args)?,
        ))
    }

    fn eval_break(&self, args: &[Expr]) -> Result<LuaResult> {
        if !args.is_empty() {
            return Err(Error::Eval("lua break accepts no operands".to_owned()));
        }
        Ok(LuaResult::break_signal())
    }

    fn eval_stdlib(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        if !args.is_empty() {
            return Err(Error::Eval("lua stdlib accepts no operands".to_owned()));
        }
        self.install_stdlib(cx, env)?;
        Ok(LuaResult::one(self.kit.nil.clone()))
    }

    fn eval_table(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        if !args.len().is_multiple_of(2) {
            return Err(Error::Eval(
                "lua table requires key/value expression pairs".to_owned(),
            ));
        }
        let mut entries = Vec::with_capacity(args.len() / 2);
        for pair in args.chunks_exact(2) {
            entries.push((
                self.eval_one(cx, env, &pair[0])?,
                self.eval_one(cx, env, &pair[1])?,
            ));
        }
        lua_table_from_values(cx, entries).map(LuaResult::one)
    }

    fn eval_get(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        let [table, key] = args else {
            return Err(Error::Eval("lua get requires table and key".to_owned()));
        };
        let table = self.eval_one(cx, env, table)?;
        let key = self.eval_one(cx, env, key)?;
        Ok(LuaResult::one(
            lua_get(cx, &table, &key)?.unwrap_or_else(|| self.kit.nil.clone()),
        ))
    }

    fn eval_rawget(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        let [table, key] = args else {
            return Err(Error::Eval("lua rawget requires table and key".to_owned()));
        };
        let table = self.eval_one(cx, env, table)?;
        let key = self.eval_one(cx, env, key)?;
        Ok(LuaResult::one(
            lua_rawget(cx, &table, &key)?.unwrap_or_else(|| self.kit.nil.clone()),
        ))
    }

    fn eval_rawset(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        let [table, key, value] = args else {
            return Err(Error::Eval(
                "lua rawset requires table, key, and value".to_owned(),
            ));
        };
        let table = self.eval_one(cx, env, table)?;
        let key = self.eval_one(cx, env, key)?;
        let value = self.eval_one(cx, env, value)?;
        lua_rawset(cx, &table, key, value.clone())?;
        Ok(LuaResult::one(value))
    }

    fn eval_len(&self, cx: &mut Cx, env: &mut LuaEnv, args: &[Expr]) -> Result<LuaResult> {
        let [value] = args else {
            return Err(Error::Eval("lua len requires one value".to_owned()));
        };
        let value = self.eval_one(cx, env, value)?;
        lua_len(cx, env, value).map(LuaResult::one)
    }

    fn eval_binary(
        &self,
        cx: &mut Cx,
        env: &mut LuaEnv,
        op: LuaOp,
        args: &[Expr],
    ) -> Result<LuaResult> {
        let [left, right] = args else {
            return Err(Error::Eval(format!(
                "lua operator {} requires two operands",
                op.name()
            )));
        };
        let left = self.eval_one(cx, env, left)?;
        let right = self.eval_one(cx, env, right)?;
        lua_binary(cx, env, op, left, right).map(LuaResult::one)
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
            LuaResult::Break => Err(Error::Eval(
                "lua break cannot be used as a value expression".to_owned(),
            )),
        }
    }

    fn eval_values(&self, cx: &mut Cx, env: &mut LuaEnv, expr: &Expr) -> Result<Vec<Value>> {
        match self.eval(cx, env, expr)? {
            LuaResult::Values(values) => {
                if let [value] = values.as_slice()
                    && let Some(values) = lua_varargs_values(value)
                {
                    return Ok(values);
                }
                Ok(values)
            }
            LuaResult::Return(_) => Err(Error::Eval(
                "lua return cannot be used as a value expression".to_owned(),
            )),
            LuaResult::Break => Err(Error::Eval(
                "lua break cannot be used as a value expression".to_owned(),
            )),
        }
    }

    fn eval_multi_exprs(
        &self,
        cx: &mut Cx,
        env: &mut LuaEnv,
        exprs: &[Expr],
    ) -> Result<Vec<Value>> {
        let Some((last, prefix)) = exprs.split_last() else {
            return Ok(Vec::new());
        };
        let mut values = Vec::with_capacity(exprs.len());
        for expr in prefix {
            values.push(self.eval_one(cx, env, expr)?);
        }
        values.extend(self.eval_values(cx, env, last)?);
        Ok(values)
    }

    fn eval_argument_values(
        &self,
        cx: &mut Cx,
        env: &mut LuaEnv,
        exprs: &[Expr],
    ) -> Result<Vec<Value>> {
        self.eval_multi_exprs(cx, env, exprs)
    }

    fn assignment_values(
        &self,
        cx: &mut Cx,
        env: &mut LuaEnv,
        exprs: &[Expr],
        count: usize,
    ) -> Result<Vec<Value>> {
        Ok(self
            .kit
            .adjust_values(self.eval_multi_exprs(cx, env, exprs)?, Arity::Exact(count)))
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
                LuaResult::Break => Err(Error::Eval(
                    "lua break cannot be used as a value expression".to_owned(),
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
