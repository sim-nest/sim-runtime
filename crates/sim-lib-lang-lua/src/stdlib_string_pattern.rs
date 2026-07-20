use std::sync::{Arc, Mutex};

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Object, ObjectCompat, Result, Symbol, Value,
};
use sim_lib_pattern::{TextLimits, TextOp, compile_lua_pattern, run_text_pattern};
use sim_lib_standard_core::Arity;

use crate::{
    LuaEvalPolicy, lua_integer_value,
    pattern_replace::{lua_gsub, replacement_from_value},
    stdlib_string::{integer_arg, string_arg},
};

pub(crate) struct LuaGMatchIterator {
    subject: String,
    ops: Vec<TextOp>,
    cursor: Mutex<usize>,
}

impl LuaGMatchIterator {
    fn new(subject: String, ops: Vec<TextOp>) -> Self {
        Self {
            subject,
            ops,
            cursor: Mutex::new(0),
        }
    }
}

impl Object for LuaGMatchIterator {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok("#<lua-gmatch-iterator>".to_owned())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LuaGMatchIterator {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for LuaGMatchIterator {
    fn call(&self, cx: &mut Cx, _args: Args) -> Result<Value> {
        let policy = LuaEvalPolicy::new(cx)?;
        let values = call_lua_gmatch_iterator(cx, &policy, self)?;
        Ok(policy
            .kit()
            .adjust_values(values, Arity::AtLeastOne)
            .into_iter()
            .next()
            .unwrap_or_else(|| policy.kit().nil.clone()))
    }
}

pub(crate) fn call_lua_gmatch_iterator(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    iterator: &LuaGMatchIterator,
) -> Result<Vec<Value>> {
    let mut cursor = iterator
        .cursor
        .lock()
        .map_err(|_| Error::PoisonedLock("lua gmatch iterator"))?;
    if *cursor > iterator.subject.len() {
        return Ok(vec![policy.kit().nil.clone()]);
    }
    let Some(matched) = run_text_pattern(
        &iterator.ops,
        &iterator.subject,
        *cursor,
        TextLimits { max_steps: 20_000 },
    ) else {
        *cursor = iterator.subject.len() + 1;
        return Ok(vec![policy.kit().nil.clone()]);
    };
    *cursor = if matched.end == matched.start {
        next_char_boundary(&iterator.subject, matched.end)
    } else {
        matched.end
    };
    capture_or_match_values(cx, &iterator.subject, &matched)
}

pub(crate) fn lua_string_find(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    let subject = string_arg(cx, &args, 0, "string.find")?;
    let pattern = string_arg(cx, &args, 1, "string.find")?;
    let init = args
        .get(2)
        .map(|value| integer_arg(cx, value, "string.find init"))
        .transpose()?
        .unwrap_or(1);
    let plain = args
        .get(3)
        .map(|value| policy.kit().is_truthy(cx, value))
        .transpose()?
        .unwrap_or(false);
    let start = lua_start_offset(&subject, init);
    let Some(matched) = (if plain {
        plain_find(&subject, &pattern, start)
    } else {
        let ops = compile_lua_pattern(&pattern)?;
        run_text_pattern(&ops, &subject, start, TextLimits { max_steps: 20_000 })
    }) else {
        return Ok(vec![policy.kit().nil.clone()]);
    };
    let mut values = vec![
        lua_integer_value(cx, matched.start as i64 + 1)?,
        lua_integer_value(cx, matched.end as i64)?,
    ];
    values.extend(capture_values(cx, &subject, &matched)?);
    Ok(values)
}

pub(crate) fn lua_string_gmatch(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let subject = string_arg(cx, &args, 0, "string.gmatch")?;
    let pattern = string_arg(cx, &args, 1, "string.gmatch")?;
    let ops = compile_lua_pattern(&pattern)?;
    cx.factory()
        .opaque(Arc::new(LuaGMatchIterator::new(subject, ops)))
        .map(|value| vec![value])
}

pub(crate) fn lua_string_gsub(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    let subject = string_arg(cx, &args, 0, "string.gsub")?;
    let pattern = string_arg(cx, &args, 1, "string.gsub")?;
    let replacement = args
        .get(2)
        .cloned()
        .ok_or_else(|| Error::Eval("string.gsub requires a replacement".to_owned()))?;
    let limit = args
        .get(3)
        .map(|value| integer_arg(cx, value, "string.gsub limit"))
        .transpose()?;
    let ops = compile_lua_pattern(&pattern)?;
    let replacement = replacement_from_value(cx, replacement)?;
    let (text, count) = lua_gsub(cx, policy, &subject, &ops, &replacement, limit)?;
    Ok(vec![
        cx.factory().string(text)?,
        lua_integer_value(cx, count)?,
    ])
}

pub(crate) fn lua_string_match(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    args: Vec<Value>,
) -> Result<Vec<Value>> {
    let subject = string_arg(cx, &args, 0, "string.match")?;
    let pattern = string_arg(cx, &args, 1, "string.match")?;
    let init = args
        .get(2)
        .map(|value| integer_arg(cx, value, "string.match init"))
        .transpose()?
        .unwrap_or(1);
    let ops = compile_lua_pattern(&pattern)?;
    let Some(matched) = run_text_pattern(
        &ops,
        &subject,
        lua_start_offset(&subject, init),
        TextLimits { max_steps: 20_000 },
    ) else {
        return Ok(vec![policy.kit().nil.clone()]);
    };
    capture_or_match_values(cx, &subject, &matched)
}

fn capture_or_match_values(
    cx: &mut Cx,
    subject: &str,
    matched: &sim_lib_pattern::TextMatch,
) -> Result<Vec<Value>> {
    let values = capture_values(cx, subject, matched)?;
    if values.is_empty() {
        return cx
            .factory()
            .string(subject[matched.start..matched.end].to_owned())
            .map(|value| vec![value]);
    }
    Ok(values)
}

fn capture_values(
    cx: &mut Cx,
    subject: &str,
    matched: &sim_lib_pattern::TextMatch,
) -> Result<Vec<Value>> {
    matched
        .captures
        .iter()
        .map(|(start, end)| {
            cx.factory()
                .string(subject.get(*start..*end).unwrap_or("").to_owned())
        })
        .collect()
}

fn plain_find(subject: &str, pattern: &str, start: usize) -> Option<sim_lib_pattern::TextMatch> {
    let offset = subject.get(start..)?.find(pattern)? + start;
    Some(sim_lib_pattern::TextMatch {
        start: offset,
        end: offset + pattern.len(),
        captures: Vec::new(),
    })
}

fn lua_start_offset(subject: &str, init: i64) -> usize {
    let raw = if init >= 0 {
        init - 1
    } else {
        subject.len() as i64 + init
    };
    clamp_char_boundary(subject, raw.max(0) as usize)
}

fn clamp_char_boundary(subject: &str, offset: usize) -> usize {
    let mut offset = offset.min(subject.len());
    while offset > 0 && !subject.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn next_char_boundary(subject: &str, cursor: usize) -> usize {
    if cursor >= subject.len() {
        return subject.len() + 1;
    }
    let mut next = cursor + 1;
    while next < subject.len() && !subject.is_char_boundary(next) {
        next += 1;
    }
    next
}
