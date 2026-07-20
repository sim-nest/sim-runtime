use sim_kernel::{Cx, Error, Expr, Result, Value};
use sim_lib_pattern::{TextLimits, TextMatch, TextOp, run_text_pattern};
use sim_lib_standard_core::Arity;

use crate::{
    LuaEvalPolicy, call::call_lua_value, lua_rawget, lua_table_value, stdlib_string::lua_to_string,
};

pub(crate) enum LuaReplacement {
    Literal(String),
    Table(Value),
    Function(Value),
}

pub(crate) fn replacement_from_value(cx: &mut Cx, value: Value) -> Result<LuaReplacement> {
    match value.object().as_expr(cx)? {
        Expr::String(text) => Ok(LuaReplacement::Literal(text)),
        _ if lua_table_value(&value).is_ok() => Ok(LuaReplacement::Table(value)),
        _ if value.object().as_callable().is_some() => Ok(LuaReplacement::Function(value)),
        _ => Err(Error::Eval(
            "string.gsub replacement must be a string, table, or function".to_owned(),
        )),
    }
}

pub(crate) fn lua_gsub(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    subject: &str,
    ops: &[TextOp],
    replacement: &LuaReplacement,
    limit: Option<i64>,
) -> Result<(String, i64)> {
    let mut out = String::new();
    let mut cursor = 0_usize;
    let mut count = 0_i64;
    let max_count = limit.unwrap_or(i64::MAX);
    while cursor <= subject.len() && count < max_count {
        let Some(matched) =
            run_text_pattern(ops, subject, cursor, TextLimits { max_steps: 20_000 })
        else {
            break;
        };
        if matched.start < cursor {
            break;
        }
        out.push_str(&subject[cursor..matched.start]);
        out.push_str(&replacement_text(
            cx,
            policy,
            subject,
            &matched,
            replacement,
        )?);
        count += 1;
        cursor = if matched.end == matched.start {
            next_char_boundary(subject, matched.end)
        } else {
            matched.end
        };
        if cursor > subject.len() {
            break;
        }
    }
    out.push_str(&subject[cursor.min(subject.len())..]);
    Ok((out, count))
}

fn replacement_text(
    cx: &mut Cx,
    policy: &LuaEvalPolicy,
    subject: &str,
    matched: &TextMatch,
    replacement: &LuaReplacement,
) -> Result<String> {
    match replacement {
        LuaReplacement::Literal(text) => expand_literal_replacement(subject, matched, text),
        LuaReplacement::Table(table) => {
            let key = first_capture_or_match(cx, subject, matched)?;
            match lua_rawget(cx, table, &key)? {
                Some(value) if !is_nil(cx, &value)? => {
                    lua_to_string(cx, &value, "gsub table value")
                }
                _ => Ok(match_text(subject, matched).to_owned()),
            }
        }
        LuaReplacement::Function(function) => {
            let values = capture_values(cx, subject, matched)?;
            let returned = call_lua_value(cx, policy, function.clone(), values)?;
            let value = policy
                .kit()
                .adjust_values(returned, Arity::AtLeastOne)
                .into_iter()
                .next()
                .unwrap_or_else(|| policy.kit().nil.clone());
            if is_nil(cx, &value)? || matches!(value.object().as_expr(cx)?, Expr::Bool(false)) {
                Ok(match_text(subject, matched).to_owned())
            } else {
                lua_to_string(cx, &value, "gsub function return")
            }
        }
    }
}

fn expand_literal_replacement(
    subject: &str,
    matched: &TextMatch,
    replacement: &str,
) -> Result<String> {
    let mut out = String::new();
    let mut chars = replacement.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        let Some(escape) = chars.next() else {
            return Err(Error::Eval(
                "string.gsub replacement has dangling '%'".to_owned(),
            ));
        };
        match escape {
            '%' => out.push('%'),
            '0' => out.push_str(match_text(subject, matched)),
            '1'..='9' => {
                let index = escape as usize - '1' as usize;
                if let Some((start, end)) = matched.captures.get(index) {
                    out.push_str(subject_range(subject, *start, *end)?);
                } else {
                    return Err(Error::Eval(
                        "string.gsub replacement references a missing capture".to_owned(),
                    ));
                }
            }
            other => {
                out.push('%');
                out.push(other);
            }
        }
    }
    Ok(out)
}

fn capture_values(cx: &mut Cx, subject: &str, matched: &TextMatch) -> Result<Vec<Value>> {
    if matched.captures.is_empty() {
        return cx
            .factory()
            .string(match_text(subject, matched).to_owned())
            .map(|value| vec![value]);
    }
    matched
        .captures
        .iter()
        .map(|(start, end)| {
            cx.factory()
                .string(subject_range(subject, *start, *end)?.to_owned())
        })
        .collect()
}

fn first_capture_or_match(cx: &mut Cx, subject: &str, matched: &TextMatch) -> Result<Value> {
    if let Some((start, end)) = matched.captures.first() {
        return cx
            .factory()
            .string(subject_range(subject, *start, *end)?.to_owned());
    }
    cx.factory().string(match_text(subject, matched).to_owned())
}

fn match_text<'a>(subject: &'a str, matched: &TextMatch) -> &'a str {
    subject_range(subject, matched.start, matched.end).unwrap_or("")
}

fn subject_range(subject: &str, start: usize, end: usize) -> Result<&str> {
    subject
        .get(start..end)
        .ok_or_else(|| Error::Eval("pattern match produced invalid string range".to_owned()))
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

fn is_nil(cx: &mut Cx, value: &Value) -> Result<bool> {
    Ok(matches!(value.object().as_expr(cx)?, Expr::Nil))
}
