use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Object, ObjectCompat, Result, Symbol, Value,
};
use sim_lib_standard_core::{Arity, SharedOrganRuntime};

use crate::{
    LuaEvalPolicy, LuaNumber, lua_core_profile, lua_float_value, lua_integer_value,
    lua_number_from_value, lua_table_from_values,
};

#[derive(Clone, Copy)]
pub(crate) enum LuaMathKind {
    Abs,
    Ceil,
    Floor,
    Max,
    Min,
    Sqrt,
    Type,
    ToInteger,
}

impl LuaMathKind {
    const ALL: [Self; 8] = [
        Self::Abs,
        Self::Ceil,
        Self::Floor,
        Self::Max,
        Self::Min,
        Self::Sqrt,
        Self::Type,
        Self::ToInteger,
    ];

    fn env_name(self) -> &'static str {
        match self {
            Self::Abs => "abs",
            Self::Ceil => "ceil",
            Self::Floor => "floor",
            Self::Max => "max",
            Self::Min => "min",
            Self::Sqrt => "sqrt",
            Self::Type => "type",
            Self::ToInteger => "tointeger",
        }
    }

    fn function_symbol(self) -> Symbol {
        Symbol::qualified("lua/math", self.env_name())
    }
}

#[derive(Clone)]
pub(crate) struct LuaMathFunction {
    kind: LuaMathKind,
}

impl LuaMathFunction {
    fn new(kind: LuaMathKind) -> Self {
        Self { kind }
    }

    pub(crate) fn kind(&self) -> LuaMathKind {
        self.kind
    }
}

impl Object for LuaMathFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-math-function {}>", self.kind.env_name()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaMathFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaMathFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = run_lua_math_function(cx, &policy, self.kind, args.into_vec())?;
        Ok(policy
            .kit()
            .adjust_values(values, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

pub(crate) fn install_lua_math_stdlib(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    env: &mut crate::LuaEnv,
) -> Result<()> {
    let mut runtime = SharedOrganRuntime::new();
    let profile = lua_core_profile();
    let profile_symbol = profile.symbol.clone();
    runtime.register_profile(profile)?;
    runtime.register_kit(&profile_symbol, policy.kit().clone())?;

    let mut entries = Vec::new();
    for kind in LuaMathKind::ALL {
        let function = cx.factory().opaque(Arc::new(LuaMathFunction::new(kind)))?;
        runtime.define_function(
            &profile_symbol,
            sim_lib_dispatch::dispatch_organ_symbol(),
            kind.function_symbol(),
            function.clone(),
        )?;
        entries.push((
            cx.factory().string(kind.env_name().to_owned())?,
            function.clone(),
        ));
        define_or_assign(
            env,
            Symbol::new(format!("math.{}", kind.env_name())),
            function,
        )?;
    }
    entries.push((
        cx.factory().string("pi".to_owned())?,
        lua_float_value(cx, std::f64::consts::PI)?,
    ));
    entries.push((
        cx.factory().string("huge".to_owned())?,
        lua_float_value(cx, f64::MAX)?,
    ));
    let table = lua_table_from_values(cx, entries)?;
    define_or_assign(env, Symbol::new("math"), table)
}

pub(crate) fn run_lua_math_function(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    kind: LuaMathKind,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    match kind {
        LuaMathKind::Abs => lua_abs(cx, args),
        LuaMathKind::Ceil => lua_ceil(cx, args),
        LuaMathKind::Floor => lua_floor(cx, args),
        LuaMathKind::Max => lua_min_max(cx, args, true),
        LuaMathKind::Min => lua_min_max(cx, args, false),
        LuaMathKind::Sqrt => lua_sqrt(cx, args),
        LuaMathKind::Type => lua_math_type(cx, policy, args),
        LuaMathKind::ToInteger => lua_tointeger(cx, policy, args),
    }
}

fn lua_abs(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    match required_number(cx, args.first(), "math.abs")? {
        LuaNumber::Integer(value) => lua_integer_value(cx, value.saturating_abs()).map(|v| vec![v]),
        LuaNumber::Float(value) => lua_float_value(cx, value.abs()).map(|v| vec![v]),
    }
}

fn lua_ceil(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let value = required_number(cx, args.first(), "math.ceil")?;
    integer_result(cx, lua_number_as_f64(value).ceil()).map(|v| vec![v])
}

fn lua_floor(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let value = required_number(cx, args.first(), "math.floor")?;
    integer_result(cx, lua_number_as_f64(value).floor()).map(|v| vec![v])
}

fn lua_min_max(cx: &mut Cx, args: Vec<Value>, max: bool) -> Result<Vec<Value>> {
    let Some(first) = args.first() else {
        return Err(Error::Eval("math.min/max requires a value".to_owned()));
    };
    let mut best = first.clone();
    let mut best_number = required_number(cx, Some(first), "math.min/max")?;
    for value in args.iter().skip(1) {
        let number = required_number(cx, Some(value), "math.min/max")?;
        let better = if max {
            lua_number_as_f64(number) > lua_number_as_f64(best_number)
        } else {
            lua_number_as_f64(number) < lua_number_as_f64(best_number)
        };
        if better {
            best = value.clone();
            best_number = number;
        }
    }
    Ok(vec![best])
}

fn lua_sqrt(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let value = required_number(cx, args.first(), "math.sqrt")?;
    lua_float_value(cx, lua_number_as_f64(value).sqrt()).map(|v| vec![v])
}

fn lua_math_type(cx: &mut Cx, policy: &LuaEvalPolicy, args: Vec<Value>) -> Result<Vec<Value>> {
    let Some(value) = args.first() else {
        return Ok(vec![policy.kit().nil.clone()]);
    };
    let Some(number) = lua_number_from_value(cx, value)? else {
        return Ok(vec![policy.kit().nil.clone()]);
    };
    let name = match number {
        LuaNumber::Integer(_) => "integer",
        LuaNumber::Float(_) => "float",
    };
    cx.factory().string(name.to_owned()).map(|v| vec![v])
}

fn lua_tointeger(cx: &mut Cx, policy: &LuaEvalPolicy, args: Vec<Value>) -> Result<Vec<Value>> {
    let Some(value) = args.first() else {
        return Ok(vec![policy.kit().nil.clone()]);
    };
    match lua_number_from_value(cx, value)? {
        Some(LuaNumber::Integer(value)) => lua_integer_value(cx, value).map(|v| vec![v]),
        Some(LuaNumber::Float(value))
            if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 =>
        {
            lua_integer_value(cx, value as i64).map(|v| vec![v])
        }
        _ => Ok(vec![policy.kit().nil.clone()]),
    }
}

fn required_number(cx: &mut Cx, value: Option<&Value>, context: &str) -> Result<LuaNumber> {
    let value = value.ok_or_else(|| Error::Eval(format!("{context} requires a number")))?;
    lua_number_from_value(cx, value)?.ok_or(Error::TypeMismatch {
        expected: "number",
        found: "non-number",
    })
}

fn integer_result(cx: &mut Cx, value: f64) -> Result<Value> {
    if value < i64::MIN as f64 || value > i64::MAX as f64 {
        return Err(Error::Eval("lua integer result is out of range".to_owned()));
    }
    lua_integer_value(cx, value as i64)
}

fn lua_number_as_f64(value: LuaNumber) -> f64 {
    match value {
        LuaNumber::Integer(value) => value as f64,
        LuaNumber::Float(value) => value,
    }
}

fn define_or_assign(env: &mut crate::LuaEnv, name: Symbol, value: Value) -> Result<()> {
    if env.contains(&name) {
        env.assign(&name, value)?;
    } else {
        env.define(name, value)?;
    }
    Ok(())
}
