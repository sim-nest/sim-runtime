//! ECMAScript JSON policy composed over `sim-codec-json` text and tree mechanics.

use std::collections::HashSet;

use sim_codec_json::JsonTree;
use sim_kernel::CodecId;

const JSON_CODEC: CodecId = CodecId(0);

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
    let parsed = sim_codec_json::parse_json(JSON_CODEC, text)
        .map_err(|error| JavascriptJsonError::Parse(codec_message(error)))?;
    let value = from_json_tree(parsed)?;
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
            let tree = to_json_tree(value, false)?;
            sim_codec_json::render_json(JSON_CODEC, &tree)
                .map(Some)
                .map_err(|error| JavascriptJsonError::Parse(codec_message(error)))
        }
    }
}
fn from_json_tree(v: JsonTree) -> Result<JavascriptJsonValue, JavascriptJsonError> {
    match v {
        JsonTree::Null => Ok(JavascriptJsonValue::Null),
        JsonTree::Bool(v) => Ok(JavascriptJsonValue::Bool(v)),
        number @ JsonTree::Number(_) => number
            .number_as_f64(JSON_CODEC)
            .map(JavascriptJsonValue::Number)
            .map_err(|error| JavascriptJsonError::Parse(codec_message(error))),
        JsonTree::String(v) => Ok(JavascriptJsonValue::String(v)),
        JsonTree::Array(v) => v
            .into_iter()
            .map(from_json_tree)
            .collect::<Result<_, _>>()
            .map(JavascriptJsonValue::Array),
        JsonTree::Object(v) => v
            .into_iter()
            .map(|(key, value)| Ok((key, from_json_tree(value)?)))
            .collect::<Result<_, _>>()
            .map(JavascriptJsonValue::Object),
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
fn to_json_tree(v: JavascriptJsonValue, in_array: bool) -> Result<JsonTree, JavascriptJsonError> {
    Ok(match v {
        JavascriptJsonValue::Null => JsonTree::Null,
        JavascriptJsonValue::Bool(v) => JsonTree::Bool(v),
        JavascriptJsonValue::Number(v) => {
            JsonTree::number_from_f64(JSON_CODEC, v).map_err(|_| JavascriptJsonError::NonFinite)?
        }
        JavascriptJsonValue::String(v) => JsonTree::String(v),
        JavascriptJsonValue::Array(v) => JsonTree::Array(
            v.into_iter()
                .map(|v| to_json_tree(v, true))
                .collect::<Result<_, _>>()?,
        ),
        JavascriptJsonValue::Object(v) => {
            let mut entries: Vec<(String, JsonTree)> = Vec::new();
            for (k, v) in v {
                if !matches!(v, JavascriptJsonValue::Undefined) {
                    let value = to_json_tree(v, false)?;
                    if let Some((_, existing)) = entries.iter_mut().find(|(key, _)| key == &k) {
                        *existing = value;
                    } else {
                        entries.push((k, value));
                    }
                }
            }
            JsonTree::Object(entries)
        }
        JavascriptJsonValue::Undefined if in_array => JsonTree::Null,
        JavascriptJsonValue::Undefined => JsonTree::Null,
    })
}

fn codec_message(error: sim_kernel::Error) -> String {
    match error {
        sim_kernel::Error::CodecError { message, .. } => message,
        error => error.to_string(),
    }
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

    #[test]
    fn reviver_deletion_distinguishes_objects_arrays_and_root() {
        let mut reviver = |key: &str, value| (key != "drop").then_some(value);
        assert_eq!(
            parse_javascript_json(r#"{"drop":1,"keep":[2]}"#, Some(&mut reviver)).unwrap(),
            JavascriptJsonValue::Object(vec![(
                "keep".into(),
                JavascriptJsonValue::Array(vec![JavascriptJsonValue::Number(2.0)]),
            )])
        );

        let mut array_reviver = |key: &str, value| (key != "0").then_some(value);
        assert_eq!(
            parse_javascript_json("[1]", Some(&mut array_reviver)).unwrap(),
            JavascriptJsonValue::Array(vec![JavascriptJsonValue::Undefined])
        );

        let mut root_reviver = |key: &str, value| (!key.is_empty()).then_some(value);
        assert_eq!(
            parse_javascript_json("null", Some(&mut root_reviver)).unwrap(),
            JavascriptJsonValue::Undefined
        );
    }

    #[test]
    fn to_json_precedes_replacer_at_every_level() {
        use std::cell::RefCell;

        let value = JavascriptJsonValue::Object(vec![(
            "item".into(),
            JavascriptJsonValue::String("before".into()),
        )]);
        let events = RefCell::new(Vec::new());
        let mut hook = |key: &str, value: &JavascriptJsonValue| {
            events.borrow_mut().push(format!("toJSON:{key}"));
            Some(value.clone())
        };
        let mut replacer = |key: &str, value| {
            events.borrow_mut().push(format!("replacer:{key}"));
            Some(value)
        };
        assert_eq!(
            stringify_javascript_json(&value, Some(&mut hook), Some(&mut replacer)).unwrap(),
            Some(r#"{"item":"before"}"#.into())
        );
        assert_eq!(
            events.into_inner(),
            ["toJSON:", "replacer:", "toJSON:item", "replacer:item"]
        );
    }

    #[test]
    fn replacer_omits_object_members_nulls_array_cells_and_omits_root() {
        let value = JavascriptJsonValue::Object(vec![
            ("gone".into(), JavascriptJsonValue::Bool(true)),
            (
                "array".into(),
                JavascriptJsonValue::Array(vec![JavascriptJsonValue::Bool(true)]),
            ),
        ]);
        let mut replacer = |key: &str, value| (key != "gone" && key != "0").then_some(value);
        assert_eq!(
            stringify_javascript_json(&value, None, Some(&mut replacer)).unwrap(),
            Some(r#"{"array":[null]}"#.into())
        );
        let mut root_replacer = |key: &str, value| (!key.is_empty()).then_some(value);
        assert_eq!(
            stringify_javascript_json(&value, None, Some(&mut root_replacer)).unwrap(),
            None
        );
    }

    #[test]
    fn number_failures_duplicates_order_diagnostics_and_exact_text_are_stable() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                stringify_javascript_json(&JavascriptJsonValue::Number(value), None, None),
                Err(JavascriptJsonError::NonFinite)
            );
        }
        let value = JavascriptJsonValue::Object(vec![
            ("4294967295".into(), JavascriptJsonValue::Number(1.0)),
            ("01".into(), JavascriptJsonValue::Number(2.0)),
            ("2".into(), JavascriptJsonValue::Number(3.0)),
            ("1".into(), JavascriptJsonValue::Number(4.0)),
            ("01".into(), JavascriptJsonValue::Number(5.0)),
        ]);
        assert_eq!(
            stringify_javascript_json(&value, None, None).unwrap(),
            Some(r#"{"1":4.0,"2":3.0,"4294967295":1.0,"01":5.0}"#.into())
        );
        assert!(matches!(
            parse_javascript_json("{]", None),
            Err(JavascriptJsonError::Parse(message)) if message.contains("line 1 column 2")
        ));
    }
}
