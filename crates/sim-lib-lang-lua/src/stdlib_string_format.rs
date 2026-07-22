use sim_kernel::{Cx, Error, Result, Value};

use crate::{LuaNumber, lua_number_from_value, stdlib_string::lua_to_string};

pub(crate) fn lua_string_format(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Value>> {
    let format = string_arg(cx, &args, 0, "string.format")?;
    let mut index = 1_usize;
    let mut out = String::new();
    let mut chars = format.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            out.push('%');
            continue;
        }
        let mut spec = String::new();
        while let Some(next) = chars.peek().copied() {
            spec.push(next);
            chars.next();
            if conversion_spec(next) {
                break;
            }
        }
        let conv = spec
            .chars()
            .last()
            .ok_or_else(|| Error::Eval("string.format has incomplete conversion".to_owned()))?;
        let value = args
            .get(index)
            .ok_or_else(|| Error::Eval("string.format missing argument".to_owned()))?;
        index += 1;
        push_formatted(cx, &mut out, &spec, conv, value)?;
    }
    cx.factory().string(out).map(|value| vec![value])
}

fn string_arg(cx: &mut Cx, args: &[Value], index: usize, context: &str) -> Result<String> {
    let value = args
        .get(index)
        .ok_or_else(|| Error::Eval(format!("{context} requires argument {}", index + 1)))?;
    lua_to_string(cx, value, context)
}

fn integer_arg(cx: &mut Cx, value: &Value, context: &str) -> Result<i64> {
    match lua_number_from_value(cx, value)? {
        Some(LuaNumber::Integer(value)) => Ok(value),
        Some(LuaNumber::Float(value)) if value.fract() == 0.0 => Ok(value as i64),
        _ => Err(Error::Eval(format!("{context} must be an integer"))),
    }
}

fn float_arg(cx: &mut Cx, value: &Value, context: &str) -> Result<f64> {
    match lua_number_from_value(cx, value)? {
        Some(LuaNumber::Integer(value)) => Ok(value as f64),
        Some(LuaNumber::Float(value)) => Ok(value),
        None => Err(Error::Eval(format!("{context} must be a number"))),
    }
}

fn push_formatted(
    cx: &mut Cx,
    out: &mut String,
    spec: &str,
    conv: char,
    value: &Value,
) -> Result<()> {
    let precision = format_precision(spec);
    match conv {
        's' => push_precision(
            out,
            lua_to_string(cx, value, "string.format %s")?,
            precision,
        ),
        'q' => out.push_str(&format!(
            "{:?}",
            lua_to_string(cx, value, "string.format %q")?
        )),
        'c' => {
            let code = integer_arg(cx, value, "string.format %c")?;
            let ch = char::from_u32(code as u32)
                .ok_or_else(|| Error::Eval("string.format %c out of range".to_owned()))?;
            out.push(ch);
        }
        'd' | 'i' => out.push_str(&integer_arg(cx, value, "string.format integer")?.to_string()),
        'u' => out.push_str(&(integer_arg(cx, value, "string.format %u")? as u64).to_string()),
        'o' => out.push_str(&format!(
            "{:o}",
            integer_arg(cx, value, "string.format %o")?
        )),
        'x' => out.push_str(&format!(
            "{:x}",
            integer_arg(cx, value, "string.format %x")?
        )),
        'X' => out.push_str(&format!(
            "{:X}",
            integer_arg(cx, value, "string.format %X")?
        )),
        'f' => {
            let value = float_arg(cx, value, "string.format %f")?;
            match precision {
                Some(precision) => out.push_str(&format!("{value:.precision$}")),
                None => out.push_str(&format!("{value:.6}")),
            }
        }
        'e' => out.push_str(&format!("{:e}", float_arg(cx, value, "string.format %e")?)),
        'E' => out.push_str(&format!("{:E}", float_arg(cx, value, "string.format %E")?)),
        'g' | 'G' => out.push_str(&float_arg(cx, value, "string.format float")?.to_string()),
        other => {
            return Err(Error::Eval(format!(
                "string.format unsupported conversion %{other}"
            )));
        }
    }
    Ok(())
}

fn conversion_spec(ch: char) -> bool {
    matches!(
        ch,
        'c' | 'd' | 'i' | 'u' | 'o' | 'x' | 'X' | 'f' | 'e' | 'E' | 'g' | 'G' | 'q' | 's'
    )
}

fn format_precision(spec: &str) -> Option<usize> {
    let dot = spec.find('.')?;
    let digits: String = spec[dot + 1..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

fn push_precision(out: &mut String, text: String, precision: Option<usize>) {
    match precision {
        Some(limit) => out.extend(text.chars().take(limit)),
        None => out.push_str(&text),
    }
}
