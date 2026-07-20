use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Expr, Object, ObjectCompat, Result, Symbol, Value,
};
use sim_lib_standard_core::{Arity, SharedOrganRuntime};

use crate::{
    LuaEvalPolicy, call::call_lua_value, lua_core_profile, lua_integer_value, lua_rawget,
    lua_rawset, lua_table_from_values,
};

#[derive(Clone, Copy)]
pub(crate) enum LuaPackageKind {
    Require,
    PreloadSearcher,
    SourceSearcher,
    CSearcher,
    AllInOneSearcher,
    SearchPath,
}

impl LuaPackageKind {
    fn env_name(self) -> &'static str {
        match self {
            Self::Require => "require",
            Self::PreloadSearcher => "preload-searcher",
            Self::SourceSearcher => "source-searcher",
            Self::CSearcher => "c-searcher",
            Self::AllInOneSearcher => "all-in-one-searcher",
            Self::SearchPath => "searchpath",
        }
    }

    fn function_symbol(self) -> Symbol {
        Symbol::qualified("lua/package", self.env_name())
    }
}

#[derive(Clone)]
pub(crate) struct LuaPackageFunction {
    kind: LuaPackageKind,
    package_table: Option<Value>,
    preload_table: Option<Value>,
}

impl LuaPackageFunction {
    fn new(kind: LuaPackageKind) -> Self {
        Self {
            kind,
            package_table: None,
            preload_table: None,
        }
    }

    fn with_package(kind: LuaPackageKind, package_table: Value) -> Self {
        Self {
            kind,
            package_table: Some(package_table),
            preload_table: None,
        }
    }

    fn with_preload(kind: LuaPackageKind, preload_table: Value) -> Self {
        Self {
            kind,
            package_table: None,
            preload_table: Some(preload_table),
        }
    }

    pub(crate) fn kind(&self) -> LuaPackageKind {
        self.kind
    }
}

impl Object for LuaPackageFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<lua-package-function {}>", self.kind.env_name()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaPackageFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaPackageFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = run_lua_package_function(cx, &policy, self, args.into_vec())?;
        Ok(policy
            .kit()
            .adjust_values(values, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

pub(crate) fn install_lua_package_stdlib(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    env: &mut crate::LuaEnv,
) -> Result<()> {
    let mut runtime = SharedOrganRuntime::new();
    let profile = lua_core_profile();
    let profile_symbol = profile.symbol.clone();
    runtime.register_profile(profile)?;
    runtime.register_kit(&profile_symbol, policy.kit().clone())?;

    let loaded = lua_table_from_values(cx, Vec::new())?;
    let preload = lua_table_from_values(cx, Vec::new())?;
    let searchers = lua_package_searchers(cx, &mut runtime, &profile_symbol, preload.clone())?;
    let searchpath = package_function(
        cx,
        &mut runtime,
        &profile_symbol,
        LuaPackageFunction::new(LuaPackageKind::SearchPath),
    )?;
    let package = lua_table_from_values(
        cx,
        vec![
            (cx.factory().string("loaded".to_owned())?, loaded),
            (cx.factory().string("preload".to_owned())?, preload),
            (cx.factory().string("searchers".to_owned())?, searchers),
            (
                cx.factory().string("path".to_owned())?,
                cx.factory().string("./?.lua;./?/init.lua".to_owned())?,
            ),
            (
                cx.factory().string("cpath".to_owned())?,
                cx.factory().string(String::new())?,
            ),
            (
                cx.factory().string("config".to_owned())?,
                cx.factory().string("/\n;\n?\n!\n-".to_owned())?,
            ),
            (
                cx.factory().string("searchpath".to_owned())?,
                searchpath.clone(),
            ),
        ],
    )?;
    let require = package_function(
        cx,
        &mut runtime,
        &profile_symbol,
        LuaPackageFunction::with_package(LuaPackageKind::Require, package.clone()),
    )?;

    define_or_assign(env, Symbol::new("package"), package)?;
    define_or_assign(env, Symbol::new("package.searchpath"), searchpath)?;
    define_or_assign(env, Symbol::new("require"), require)
}

pub(crate) fn run_lua_package_function(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    function: &LuaPackageFunction,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    match function.kind {
        LuaPackageKind::Require => lua_require(cx, policy, function, args),
        LuaPackageKind::PreloadSearcher => lua_preload_searcher(cx, policy, function, args),
        LuaPackageKind::SourceSearcher
        | LuaPackageKind::CSearcher
        | LuaPackageKind::AllInOneSearcher => lua_gap_searcher(cx, policy, function.kind, args),
        LuaPackageKind::SearchPath => lua_searchpath(cx, policy, args),
    }
}

fn lua_package_searchers(
    cx: &mut Cx,
    runtime: &mut SharedOrganRuntime,
    profile_symbol: &Symbol,
    preload: Value,
) -> Result<Value> {
    let kinds = [
        LuaPackageFunction::with_preload(LuaPackageKind::PreloadSearcher, preload),
        LuaPackageFunction::new(LuaPackageKind::SourceSearcher),
        LuaPackageFunction::new(LuaPackageKind::CSearcher),
        LuaPackageFunction::new(LuaPackageKind::AllInOneSearcher),
    ];
    let mut entries = Vec::new();
    for (index, function) in kinds.into_iter().enumerate() {
        entries.push((
            lua_integer_value(cx, index as i64 + 1)?,
            package_function(cx, runtime, profile_symbol, function)?,
        ));
    }
    lua_table_from_values(cx, entries)
}

fn package_function(
    cx: &mut Cx,
    runtime: &mut SharedOrganRuntime,
    profile_symbol: &Symbol,
    function: LuaPackageFunction,
) -> Result<Value> {
    let kind = function.kind();
    let value = cx.factory().opaque(Arc::new(function))?;
    runtime.define_function(
        profile_symbol,
        sim_lib_dispatch::dispatch_organ_symbol(),
        kind.function_symbol(),
        value.clone(),
    )?;
    Ok(value)
}

fn lua_require(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    function: &LuaPackageFunction,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    let module = string_arg(cx, &args, 0, "require module")?;
    let package = function
        .package_table
        .as_ref()
        .ok_or_else(|| Error::Eval("require is missing package table".to_owned()))?;
    let loaded = table_field(cx, package, "loaded")?;
    let module_key = cx.factory().string(module.clone())?;
    let loaded_value = lua_rawget(cx, &loaded, &module_key)?;
    if let Some(value) = loaded_value
        && !matches!(value.object().as_expr(cx)?, Expr::Nil)
    {
        return Ok(vec![value]);
    }

    let searchers = table_field(cx, package, "searchers")?;
    let mut notes = Vec::new();
    for index in 1..=4 {
        let key = lua_integer_value(cx, index)?;
        let Some(searcher) = lua_rawget(cx, &searchers, &key)? else {
            continue;
        };
        let found = call_lua_value(
            cx,
            policy,
            searcher,
            vec![cx.factory().string(module.clone())?],
        )?;
        match found.as_slice() {
            [loader, ..] if loader.object().as_callable().is_some() => {
                let mut values = call_lua_value(
                    cx,
                    policy,
                    loader.clone(),
                    vec![cx.factory().string(module.clone())?],
                )?;
                let result = values
                    .drain(..1)
                    .next()
                    .filter(|value| !matches!(value.object().as_expr(cx), Ok(Expr::Nil)))
                    .unwrap_or_else(|| cx.factory().bool(true).unwrap());
                cache_loaded(cx, &loaded, module_key, result.clone())?;
                return Ok(vec![result]);
            }
            [nil, note, ..] if matches!(nil.object().as_expr(cx)?, Expr::Nil) => {
                notes.push(note.object().display(cx)?);
            }
            _ => {}
        }
    }
    Err(Error::Eval(format!(
        "module '{module}' not found{}",
        if notes.is_empty() {
            String::new()
        } else {
            format!(": {}", notes.join("; "))
        }
    )))
}

fn lua_preload_searcher(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    function: &LuaPackageFunction,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    let module = string_arg(cx, &args, 0, "package preload searcher module")?;
    let preload = function
        .preload_table
        .as_ref()
        .ok_or_else(|| Error::Eval("preload searcher is missing preload table".to_owned()))?;
    let key = cx.factory().string(module.clone())?;
    match lua_rawget(cx, preload, &key)? {
        Some(loader) => Ok(vec![loader]),
        None => Ok(vec![
            policy.kit().nil.clone(),
            cx.factory()
                .string(format!("no field package.preload['{module}']"))?,
        ]),
    }
}

fn lua_gap_searcher(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    kind: LuaPackageKind,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    let module = string_arg(cx, &args, 0, "package searcher module")?;
    let lane = kind.env_name();
    Ok(vec![
        policy.kit().nil.clone(),
        cx.factory().string(format!(
            "{lane} has no loadable package source for '{module}'"
        ))?,
    ])
}

fn lua_searchpath(cx: &mut Cx, policy: &LuaEvalPolicy, args: Vec<Value>) -> Result<Vec<Value>> {
    let module = string_arg(cx, &args, 0, "package.searchpath module")?;
    Ok(vec![
        policy.kit().nil.clone(),
        cx.factory()
            .string(format!("no Lua package path resolved for '{module}'"))?,
    ])
}

fn cache_loaded(cx: &mut Cx, loaded: &Value, key: Value, value: Value) -> Result<()> {
    match lua_rawset(cx, loaded, key, value) {
        Ok(()) | Err(Error::CapabilityDenied { .. }) => Ok(()),
        Err(err) => Err(err),
    }
}

fn table_field(cx: &mut Cx, table: &Value, field: &str) -> Result<Value> {
    let key = cx.factory().string(field.to_owned())?;
    lua_rawget(cx, table, &key)?.ok_or_else(|| Error::Eval(format!("package.{field} missing")))
}

fn string_arg(cx: &mut Cx, args: &[Value], index: usize, context: &str) -> Result<String> {
    let value = args
        .get(index)
        .ok_or_else(|| Error::Eval(format!("{context} requires a string")))?;
    match value.object().as_expr(cx)? {
        Expr::String(text) => Ok(text),
        _ => Err(Error::TypeMismatch {
            expected: "string",
            found: "non-string",
        }),
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
