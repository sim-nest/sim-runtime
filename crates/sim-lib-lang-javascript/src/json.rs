//! ECMAScript JSON policy over the canonical JSON representation.

use std::collections::HashSet;

/// JavaScript JSON-domain value. Objects retain insertion order until the
/// ECMAScript property-order projection is applied.
#[derive(Clone, Debug, PartialEq)]
pub enum JavascriptJsonValue {
    /// JSON null.
    Null,
    /// Boolean.
    Bool(bool),
    /// Number.
    Number(f64),
    /// String.
    String(String),
    /// Array.
    Array(Vec<JavascriptJsonValue>),
    /// Object as insertion-ordered own string properties.
    Object(Vec<(String, JavascriptJsonValue)>),
    /// Value omitted by object serialization or rendered as null in arrays.
    Undefined,
}
/// JSON policy error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JavascriptJsonError {
    /// Canonical parser rejected the input.
    Parse(String),
    /// A cycle was found during stringify.
    Cycle,
    /// Number is not representable by JSON.
    NonFinite,
}
/// Reviver callback, invoked bottom-up with the empty root key last.
pub type JsonReviver<'a> = dyn FnMut(&str, JavascriptJsonValue) -> Option<JavascriptJsonValue> + 'a;
/// Replacer callback, invoked top-down with the empty root key first.
pub type JsonReplacer<'a> =
    dyn FnMut(&str, JavascriptJsonValue) -> Option<JavascriptJsonValue> + 'a;
/// JavaScript `toJSON` hook invoked before the replacer.
pub type JsonToJson<'a> = dyn FnMut(&str, &JavascriptJsonValue) -> Option<JavascriptJsonValue> + 'a;

/// Parse through the canonical JSON parser, then apply ECMAScript reviver order.
pub fn parse_javascript_json(
    text: &str,
    mut reviver: Option<&mut JsonReviver<'_>>,
) -> Result<JavascriptJsonValue, JavascriptJsonError> {
    let parsed: serde_json::Value =
        serde_json::from_str(text).map_err(|e| JavascriptJsonError::Parse(e.to_string()))?;
    let expr = sim_codec_json::project_json_to_expr(
        &parsed,
        sim_codec_json::JsonProjectionMode::UntaggedInterop,
    );
    let canonical = sim_codec_json::project_expr_to_json(
        &expr,
        sim_codec_json::JsonProjectionMode::UntaggedInterop,
    );
    let value = from_canonical(canonical);
    Ok(if let Some(callback) = reviver.as_mut() {
        walk_reviver("", value, *callback).unwrap_or(JavascriptJsonValue::Undefined)
    } else {
        value
    })
}
/// Serialize with `toJSON`, replacer, ECMAScript own-property order, and cycle rejection.
pub fn stringify_javascript_json(
    value: &JavascriptJsonValue,
    mut to_json: Option<&mut JsonToJson<'_>>,
    mut replacer: Option<&mut JsonReplacer<'_>>,
) -> Result<Option<String>, JavascriptJsonError> {
    let mut ancestors = HashSet::new();
    let projected = project(
        "",
        value.clone(),
        &mut to_json,
        &mut replacer,
        &mut ancestors,
        false,
    )?;
    match projected {
        None | Some(JavascriptJsonValue::Undefined) => Ok(None),
        Some(value) => {
            let json = to_canonical(value, false)?;
            let expr = sim_codec_json::project_json_to_expr(
                &json,
                sim_codec_json::JsonProjectionMode::UntaggedInterop,
            );
            let canonical = sim_codec_json::project_expr_to_json(
                &expr,
                sim_codec_json::JsonProjectionMode::UntaggedInterop,
            );
            serde_json::to_string(&canonical)
                .map(Some)
                .map_err(|e| JavascriptJsonError::Parse(e.to_string()))
        }
    }
}
fn from_canonical(v: serde_json::Value) -> JavascriptJsonValue {
    match v {
        serde_json::Value::Null => JavascriptJsonValue::Null,
        serde_json::Value::Bool(v) => JavascriptJsonValue::Bool(v),
        serde_json::Value::Number(v) => JavascriptJsonValue::Number(v.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(v) => JavascriptJsonValue::String(v),
        serde_json::Value::Array(v) => {
            JavascriptJsonValue::Array(v.into_iter().map(from_canonical).collect())
        }
        serde_json::Value::Object(v) => JavascriptJsonValue::Object(
            v.into_iter().map(|(k, v)| (k, from_canonical(v))).collect(),
        ),
    }
}
fn walk_reviver(
    key: &str,
    value: JavascriptJsonValue,
    reviver: &mut JsonReviver<'_>,
) -> Option<JavascriptJsonValue> {
    let walked = match value {
        JavascriptJsonValue::Array(values) => JavascriptJsonValue::Array(
            values
                .into_iter()
                .enumerate()
                .map(|(i, v)| {
                    walk_reviver(&i.to_string(), v, reviver)
                        .unwrap_or(JavascriptJsonValue::Undefined)
                })
                .collect(),
        ),
        JavascriptJsonValue::Object(entries) => JavascriptJsonValue::Object(
            entries
                .into_iter()
                .filter_map(|(k, v)| walk_reviver(&k, v, reviver).map(|v| (k, v)))
                .collect(),
        ),
        v => v,
    };
    reviver(key, walked)
}
fn project(
    key: &str,
    mut value: JavascriptJsonValue,
    to_json: &mut Option<&mut JsonToJson<'_>>,
    replacer: &mut Option<&mut JsonReplacer<'_>>,
    ancestors: &mut HashSet<usize>,
    in_array: bool,
) -> Result<Option<JavascriptJsonValue>, JavascriptJsonError> {
    if let Some(hook) = to_json.as_mut()
        && let Some(v) = hook(key, &value)
    {
        value = v;
    }
    if let Some(callback) = replacer.as_mut() {
        let Some(v) = callback(key, value) else {
            return Ok(None);
        };
        value = v;
    }
    match value {
        JavascriptJsonValue::Array(values) => {
            let identity = values.as_ptr() as usize;
            if !ancestors.insert(identity) {
                return Err(JavascriptJsonError::Cycle);
            }
            let mut out = Vec::with_capacity(values.len());
            for (i, v) in values.into_iter().enumerate() {
                out.push(
                    project(&i.to_string(), v, to_json, replacer, ancestors, true)?
                        .unwrap_or(JavascriptJsonValue::Null),
                );
            }
            ancestors.remove(&identity);
            Ok(Some(JavascriptJsonValue::Array(out)))
        }
        JavascriptJsonValue::Object(entries) => {
            let identity = entries.as_ptr() as usize;
            if !ancestors.insert(identity) {
                return Err(JavascriptJsonError::Cycle);
            }
            let mut out = Vec::new();
            for (k, v) in ordered_entries(entries) {
                if let Some(v) = project(&k, v, to_json, replacer, ancestors, false)?
                    && !matches!(v, JavascriptJsonValue::Undefined)
                {
                    out.push((k, v));
                }
            }
            ancestors.remove(&identity);
            Ok(Some(JavascriptJsonValue::Object(out)))
        }
        JavascriptJsonValue::Undefined if in_array => Ok(Some(JavascriptJsonValue::Null)),
        JavascriptJsonValue::Undefined => Ok(None),
        v => Ok(Some(v)),
    }
}
fn ordered_entries(
    entries: Vec<(String, JavascriptJsonValue)>,
) -> Vec<(String, JavascriptJsonValue)> {
    let mut indexed = Vec::new();
    let mut named = Vec::new();
    for (order, (key, value)) in entries.into_iter().enumerate() {
        if let Some(index) = array_index(&key) {
            indexed.push((index, order, key, value));
        } else {
            named.push((order, key, value));
        }
    }
    indexed.sort_by_key(|v| v.0);
    indexed
        .into_iter()
        .map(|(_, _, k, v)| (k, v))
        .chain(named.into_iter().map(|(_, k, v)| (k, v)))
        .collect()
}
fn array_index(key: &str) -> Option<u32> {
    let value = key.parse::<u32>().ok()?;
    if value == u32::MAX || value.to_string() != key {
        return None;
    }
    Some(value)
}
fn to_canonical(
    v: JavascriptJsonValue,
    in_array: bool,
) -> Result<serde_json::Value, JavascriptJsonError> {
    Ok(match v {
        JavascriptJsonValue::Null => serde_json::Value::Null,
        JavascriptJsonValue::Bool(v) => serde_json::Value::Bool(v),
        JavascriptJsonValue::Number(v) => serde_json::Number::from_f64(v)
            .map(serde_json::Value::Number)
            .ok_or(JavascriptJsonError::NonFinite)?,
        JavascriptJsonValue::String(v) => serde_json::Value::String(v),
        JavascriptJsonValue::Array(v) => serde_json::Value::Array(
            v.into_iter()
                .map(|v| to_canonical(v, true))
                .collect::<Result<_, _>>()?,
        ),
        JavascriptJsonValue::Object(v) => {
            let mut map = serde_json::Map::new();
            for (k, v) in v {
                if !matches!(v, JavascriptJsonValue::Undefined) {
                    map.insert(k, to_canonical(v, false)?);
                }
            }
            serde_json::Value::Object(map)
        }
        JavascriptJsonValue::Undefined if in_array => serde_json::Value::Null,
        JavascriptJsonValue::Undefined => serde_json::Value::Null,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reviver_is_bottom_up() {
        let mut keys = Vec::new();
        let mut r = |key: &str, v| {
            keys.push(key.to_owned());
            Some(v)
        };
        parse_javascript_json(r#"{"a":[1]}"#, Some(&mut r)).unwrap();
        assert_eq!(keys, vec!["0", "a", ""]);
    }
    #[test]
    fn replacer_to_json_and_property_order_compose() {
        let value = JavascriptJsonValue::Object(vec![
            ("b".into(), JavascriptJsonValue::Number(1.)),
            ("10".into(), JavascriptJsonValue::Number(10.)),
            ("2".into(), JavascriptJsonValue::Number(2.)),
        ]);
        let mut hook = |_: &str, v: &JavascriptJsonValue| Some(v.clone());
        let mut replace = |k: &str, v| if k == "b" { None } else { Some(v) };
        assert_eq!(
            stringify_javascript_json(&value, Some(&mut hook), Some(&mut replace))
                .unwrap()
                .unwrap(),
            r#"{"2":2.0,"10":10.0}"#
        );
    }
    #[test]
    fn undefined_policy_matches_arrays_and_objects() {
        let a = JavascriptJsonValue::Array(vec![JavascriptJsonValue::Undefined]);
        assert_eq!(
            stringify_javascript_json(&a, None, None).unwrap(),
            Some("[null]".into())
        );
        assert_eq!(
            stringify_javascript_json(&JavascriptJsonValue::Undefined, None, None).unwrap(),
            None
        );
    }
}
