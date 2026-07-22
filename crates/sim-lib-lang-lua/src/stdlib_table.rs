use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value,
};
use sim_lib_standard_core::{Arity, SharedOrganRuntime};

use crate::{
    LuaEvalPolicy, LuaNumber, call::call_lua_value, lua_core_profile, lua_integer_value,
    lua_number_from_value, lua_rawdel, lua_rawget, lua_rawset, lua_table_from_values,
    lua_table_value,
};

#[derive(Clone, Copy)]
pub(crate) enum LuaTableKind {
    Insert,
    Remove,
    Move,
    Concat,
    Sort,
    Pack,
    Unpack,
}

impl LuaTableKind {
    const ALL: [Self; 7] = [
        Self::Insert,
        Self::Remove,
        Self::Move,
        Self::Concat,
        Self::Sort,
        Self::Pack,
        Self::Unpack,
    ];

    fn env_name(self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::Remove => "remove",
            Self::Move => "move",
            Self::Concat => "concat",
            Self::Sort => "sort",
            Self::Pack => "pack",
            Self::Unpack => "unpack",
        }
    }

    fn function_symbol(self) -> Symbol {
        Symbol::qualified("lua/table", self.env_name())
    }

    fn organ(self) -> Symbol {
        match self {
            Self::Concat | Self::Pack | Self::Unpack => sim_lib_sequence::sequence_organ_symbol(),
            Self::Insert | Self::Remove | Self::Move | Self::Sort => {
                sim_lib_mutation::mutation_organ_symbol()
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct LuaTableFunction {
    kind: LuaTableKind,
}

impl LuaTableFunction {
    fn new(kind: LuaTableKind) -> Self {
        Self { kind }
    }

    pub(crate) fn kind(&self) -> LuaTableKind {
        self.kind
    }
}

impl Object for LuaTableFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-table-function {}>", self.kind.env_name()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaTableFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaTableFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = run_lua_table_function(cx, &policy, self.kind, args.into_vec())?;
        Ok(policy
            .kit()
            .adjust_values(values, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

pub(crate) fn install_lua_table_stdlib(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    env: &mut crate::LuaEnv,
) -> Result<()> {
    let mut runtime = SharedOrganRuntime::new();
    let profile = lua_core_profile();
    let profile_symbol = profile.symbol.clone();
    runtime.register_profile(profile)?;
    runtime.register_kit(&profile_symbol, policy.kit().clone())?;

    let mut table_entries = Vec::new();
    for kind in LuaTableKind::ALL {
        let function = cx.factory().opaque(Arc::new(LuaTableFunction::new(kind)))?;
        runtime.define_function(
            &profile_symbol,
            kind.organ(),
            kind.function_symbol(),
            function.clone(),
        )?;
        table_entries.push((
            cx.factory().string(kind.env_name().to_owned())?,
            function.clone(),
        ));
        define_or_assign(
            env,
            Symbol::new(format!("table.{}", kind.env_name())),
            function,
        )?;
    }
    let table = lua_table_from_values(cx, table_entries)?;
    define_or_assign(env, Symbol::new("table"), table)
}

pub(crate) fn run_lua_table_function(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    kind: LuaTableKind,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    match kind {
        LuaTableKind::Insert => lua_table_insert(cx, policy, args),
        LuaTableKind::Remove => lua_table_remove(cx, policy, args),
        LuaTableKind::Move => lua_table_move(cx, args),
        LuaTableKind::Concat => lua_table_concat(cx, args),
        LuaTableKind::Sort => lua_table_sort(cx, policy, args),
        LuaTableKind::Pack => lua_table_pack(cx, args),
        LuaTableKind::Unpack => lua_table_unpack(cx, policy, args),
    }
}

fn lua_table_insert(cx: &mut Cx, policy: &LuaEvalPolicy, args: Vec<Value>) -> Result<Vec<Value>> {
    let (table, pos, value) = match args.as_slice() {
        [table, value] => {
            let len = lua_table_value(table)?.len_border(cx)?;
            (table.clone(), len + 1, value.clone())
        }
        [table, pos, value] => (
            table.clone(),
            integer_arg(cx, pos, "table.insert position")?,
            value.clone(),
        ),
        _ => {
            return Err(Error::Eval(
                "table.insert requires table, optional position, and value".to_owned(),
            ));
        }
    };
    let len = lua_table_value(&table)?.len_border(cx)?;
    if pos < 1 || pos > len + 1 {
        return Err(Error::Eval("table.insert position out of range".to_owned()));
    }
    for index in (pos..=len).rev() {
        move_slot(cx, policy, &table, index, index + 1)?;
    }
    raw_set_index(cx, &table, pos, value)?;
    Ok(Vec::new())
}

fn lua_table_remove(cx: &mut Cx, policy: &LuaEvalPolicy, args: Vec<Value>) -> Result<Vec<Value>> {
    let table = first_arg(&args, "table.remove")?.clone();
    let len = lua_table_value(&table)?.len_border(cx)?;
    let pos = match args.get(1) {
        Some(value) => integer_arg(cx, value, "table.remove position")?,
        None => len,
    };
    if len == 0 || pos < 1 || pos > len {
        return Ok(vec![policy.kit().nil.clone()]);
    }
    let removed = raw_get_index(cx, &table, pos)?.unwrap_or_else(|| policy.kit().nil.clone());
    for index in pos + 1..=len {
        move_slot(cx, policy, &table, index, index - 1)?;
    }
    raw_del_index(cx, &table, len)?;
    Ok(vec![removed])
}

fn lua_table_move(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let [source, first, last, target, rest @ ..] = args.as_slice() else {
        return Err(Error::Eval(
            "table.move requires source, first, last, and target".to_owned(),
        ));
    };
    let first = integer_arg(cx, first, "table.move first")?;
    let last = integer_arg(cx, last, "table.move last")?;
    let target_start = integer_arg(cx, target, "table.move target")?;
    let destination = rest.first().cloned().unwrap_or_else(|| source.clone());
    if first <= last {
        let mut values = Vec::new();
        for index in first..=last {
            values.push(
                raw_get_index(cx, source, index)?.unwrap_or_else(|| cx.factory().nil().unwrap()),
            );
        }
        for (offset, value) in values.into_iter().enumerate() {
            raw_set_index(cx, &destination, target_start + offset as i64, value)?;
        }
    }
    Ok(vec![destination])
}

fn lua_table_concat(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let table = first_arg(&args, "table.concat")?;
    let sep = args
        .get(1)
        .map(|value| string_arg(cx, value, "table.concat separator"))
        .transpose()?
        .unwrap_or_default();
    let len = lua_table_value(table)?.len_border(cx)?;
    let first = match args.get(2) {
        Some(value) => integer_arg(cx, value, "table.concat first")?,
        None => 1,
    };
    let last = match args.get(3) {
        Some(value) => integer_arg(cx, value, "table.concat last")?,
        None => len,
    };
    let mut parts = Vec::new();
    if first <= last {
        for index in first..=last {
            let value = raw_get_index(cx, table, index)?
                .ok_or_else(|| Error::Eval("table.concat found nil array slot".to_owned()))?;
            parts.push(lua_string_coercion(cx, &value, "table.concat value")?);
        }
    }
    cx.factory()
        .string(parts.join(&sep))
        .map(|value| vec![value])
}

fn lua_table_sort(cx: &mut Cx, policy: &LuaEvalPolicy, args: Vec<Value>) -> Result<Vec<Value>> {
    let table = first_arg(&args, "table.sort")?.clone();
    let comparator = args.get(1).cloned();
    let len = lua_table_value(&table)?.len_border(cx)?;
    let mut values = Vec::new();
    for index in 1..=len {
        values.push(
            raw_get_index(cx, &table, index)?
                .ok_or_else(|| Error::Eval("table.sort found nil array slot".to_owned()))?,
        );
    }
    for index in 1..values.len() {
        let mut cursor = index;
        while cursor > 0
            && lua_less_than(
                cx,
                policy,
                &values[cursor],
                &values[cursor - 1],
                comparator.as_ref(),
            )?
        {
            values.swap(cursor, cursor - 1);
            cursor -= 1;
        }
    }
    for (index, value) in values.into_iter().enumerate() {
        raw_set_index(cx, &table, index as i64 + 1, value)?;
    }
    Ok(Vec::new())
}

fn lua_table_pack(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let mut entries = Vec::with_capacity(args.len() + 1);
    for (index, value) in args.iter().cloned().enumerate() {
        entries.push((lua_integer_value(cx, index as i64 + 1)?, value));
    }
    entries.push((
        cx.factory().string("n".to_owned())?,
        lua_integer_value(cx, args.len() as i64)?,
    ));
    lua_table_from_values(cx, entries).map(|value| vec![value])
}

fn lua_table_unpack(cx: &mut Cx, policy: &LuaEvalPolicy, args: Vec<Value>) -> Result<Vec<Value>> {
    let table = first_arg(&args, "table.unpack")?;
    let len = lua_table_value(table)?.len_border(cx)?;
    let first = match args.get(1) {
        Some(value) => integer_arg(cx, value, "table.unpack first")?,
        None => 1,
    };
    let last = match args.get(2) {
        Some(value) => integer_arg(cx, value, "table.unpack last")?,
        None => len,
    };
    let mut values = Vec::new();
    if first <= last {
        for index in first..=last {
            values
                .push(raw_get_index(cx, table, index)?.unwrap_or_else(|| policy.kit().nil.clone()));
        }
    }
    Ok(values)
}

fn move_slot(cx: &mut Cx, policy: &LuaEvalPolicy, table: &Value, from: i64, to: i64) -> Result<()> {
    match raw_get_index(cx, table, from)? {
        Some(value) => raw_set_index(cx, table, to, value),
        None => raw_set_index(cx, table, to, policy.kit().nil.clone()),
    }
}

fn lua_less_than(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    left: &Value,
    right: &Value,
    comparator: Option<&Value>,
) -> Result<bool> {
    if let Some(comparator) = comparator {
        let result = call_lua_value(
            cx,
            policy,
            comparator.clone(),
            vec![left.clone(), right.clone()],
        )?;
        let value = policy
            .kit()
            .adjust_values(result, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone());
        return policy.kit().is_truthy(cx, &value);
    }
    if let (Some(left), Some(right)) = (
        lua_number_from_value(cx, left)?,
        lua_number_from_value(cx, right)?,
    ) {
        return Ok(number_as_f64(left) < number_as_f64(right));
    }
    Ok(string_arg(cx, left, "table.sort value")? < string_arg(cx, right, "table.sort value")?)
}

fn raw_get_index(cx: &mut Cx, table: &Value, index: i64) -> Result<Option<Value>> {
    let key = lua_integer_value(cx, index)?;
    lua_rawget(cx, table, &key)
}

fn raw_set_index(cx: &mut Cx, table: &Value, index: i64, value: Value) -> Result<()> {
    let key = lua_integer_value(cx, index)?;
    lua_rawset(cx, table, key, value)
}

fn raw_del_index(cx: &mut Cx, table: &Value, index: i64) -> Result<()> {
    let key = lua_integer_value(cx, index)?;
    lua_rawdel(cx, table, &key)?;
    Ok(())
}

fn first_arg<'a>(args: &'a [Value], context: &str) -> Result<&'a Value> {
    args.first()
        .ok_or_else(|| Error::Eval(format!("{context} requires a table")))
}

fn integer_arg(cx: &mut Cx, value: &Value, context: &str) -> Result<i64> {
    match lua_number_from_value(cx, value)? {
        Some(LuaNumber::Integer(value)) => Ok(value),
        Some(LuaNumber::Float(value)) if value.fract() == 0.0 => Ok(value as i64),
        _ => Err(Error::Eval(format!("{context} must be an integer"))),
    }
}

fn string_arg(cx: &mut Cx, value: &Value, context: &str) -> Result<String> {
    match value.object().as_expr(cx)? {
        Expr::String(value) => Ok(value),
        _ => Err(Error::Eval(format!("{context} must be a string"))),
    }
}

fn lua_string_coercion(cx: &mut Cx, value: &Value, context: &str) -> Result<String> {
    match value.object().as_expr(cx)? {
        Expr::String(value) => Ok(value),
        Expr::Number(number) => Ok(number.canonical),
        _ => Err(Error::Eval(format!("{context} must be a string or number"))),
    }
}

fn number_as_f64(value: LuaNumber) -> f64 {
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
