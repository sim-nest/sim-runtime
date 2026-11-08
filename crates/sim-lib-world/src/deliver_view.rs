// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Revision-fenced text of one delivery qualification.
//!
//! Qualification remains the only executor. Status and why import this text
//! and refuse a missing, altered, or wrong-realization body instead of
//! constructing another journal.

use sim_kernel::{Datum, NumberLiteral, Symbol};

use crate::WorldError;
use crate::command::render;
use crate::provider;

const HEADER: &str = "sim-world-delivery-view-v1";

pub(super) fn export(record: &Datum) -> Result<String, WorldError> {
    let body = render(record);
    if body.contains(['\n', '\r']) {
        return Err(WorldError::Qualification(
            "delivery view is not a single line".to_owned(),
        ));
    }
    let digest = revision_of(&body)?;
    Ok(format!("{HEADER} {digest} {body}"))
}

pub(super) fn open(text: &str) -> Result<Option<Datum>, WorldError> {
    let Some((stated, body)) = split(text) else {
        return Ok(None);
    };
    if stated != revision_of(body)? {
        return Ok(None);
    }
    Ok(parse_datum(body))
}

fn revision_of(body: &str) -> Result<String, WorldError> {
    let id = provider::content_id(Datum::String(body.to_owned()))?;
    Ok(hex(&id.bytes))
}

fn split(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix(HEADER)?;
    let rest = rest.strip_prefix(' ')?;
    let (revision, body) = rest.split_once(' ')?;
    if revision.len() != 64 || !revision.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    if body.is_empty() {
        return None;
    }
    Some((revision, body))
}

fn parse_datum(text: &str) -> Option<Datum> {
    let mut cursor = Cursor { text, at: 0 };
    let value = parse_value(&mut cursor)?;
    cursor.skip();
    if cursor.rest().is_empty() {
        Some(value)
    } else {
        None
    }
}

struct Cursor<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Cursor<'a> {
    fn rest(&self) -> &'a str {
        &self.text[self.at..]
    }

    fn skip(&mut self) {
        let offset = self
            .rest()
            .bytes()
            .take_while(|byte| byte.is_ascii_whitespace())
            .count();
        self.at += offset;
    }

    fn eat(&mut self, expected: &str) -> bool {
        self.skip();
        if self.rest().starts_with(expected) {
            self.at += expected.len();
            true
        } else {
            false
        }
    }

    fn symbol(&mut self) -> Option<String> {
        self.skip();
        let end = self
            .rest()
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '(' | ')' | '[' | ']'))
            .unwrap_or(self.rest().len());
        if end == 0 {
            return None;
        }
        let symbol = self.rest()[..end].to_owned();
        self.at += end;
        Some(symbol)
    }

    fn keyword(&mut self, word: &str) -> bool {
        self.skip();
        let rest = self.rest();
        if !rest.starts_with(word) {
            return false;
        }
        let boundary = rest[word.len()..].chars().next();
        if boundary.is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '/')) {
            return false;
        }
        self.at += word.len();
        true
    }

    fn string(&mut self) -> Option<String> {
        if !self.eat("\"") {
            return None;
        }
        let rest = self.rest();
        let mut out = String::new();
        let mut chars = rest.char_indices();
        while let Some((index, ch)) = chars.next() {
            if ch == '\\' {
                let (_, next) = chars.next()?;
                match next {
                    '\\' => out.push('\\'),
                    '"' => out.push('"'),
                    _ => return None,
                }
                continue;
            }
            if ch == '"' {
                self.at += index + ch.len_utf8();
                return Some(out);
            }
            out.push(ch);
        }
        None
    }

    fn integer(&mut self) -> Option<u64> {
        self.skip();
        let end = self.rest().bytes().take_while(u8::is_ascii_digit).count();
        if end == 0 {
            return None;
        }
        let value = self.rest()[..end].parse().ok()?;
        self.at += end;
        Some(value)
    }
}

fn parse_value(cursor: &mut Cursor<'_>) -> Option<Datum> {
    cursor.skip();
    if cursor.keyword("true") {
        return Some(Datum::Bool(true));
    }
    if cursor.keyword("false") {
        return Some(Datum::Bool(false));
    }
    if cursor.keyword("nil") {
        return Some(Datum::Nil);
    }
    if cursor.rest().starts_with("#(") {
        return parse_node(cursor);
    }
    if cursor.rest().starts_with('"') {
        return Some(Datum::String(cursor.string()?));
    }
    if cursor.rest().starts_with('[') {
        return parse_vector(cursor);
    }
    if cursor.rest().starts_with('(') {
        return parse_list(cursor);
    }
    if cursor
        .rest()
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
    {
        return Some(count(cursor.integer()?));
    }
    None
}

fn parse_node(cursor: &mut Cursor<'_>) -> Option<Datum> {
    if !cursor.eat("#(") {
        return None;
    }
    let tag = symbol_from(cursor.symbol()?);
    let mut fields = Vec::new();
    loop {
        cursor.skip();
        if cursor.rest().starts_with(')') {
            if !cursor.eat(")") {
                return None;
            }
            return Some(Datum::Node { tag, fields });
        }
        if !cursor.eat("(") {
            return None;
        }
        let name = symbol_from(cursor.symbol()?);
        let value = parse_value(cursor)?;
        if !cursor.eat(")") {
            return None;
        }
        fields.push((name, value));
    }
}

fn parse_vector(cursor: &mut Cursor<'_>) -> Option<Datum> {
    if !cursor.eat("[") {
        return None;
    }
    let mut items = Vec::new();
    loop {
        cursor.skip();
        if cursor.rest().starts_with(']') {
            if !cursor.eat("]") {
                return None;
            }
            return Some(Datum::Vector(items));
        }
        items.push(parse_value(cursor)?);
    }
}

fn parse_list(cursor: &mut Cursor<'_>) -> Option<Datum> {
    if !cursor.eat("(") {
        return None;
    }
    let mut items = Vec::new();
    loop {
        cursor.skip();
        if cursor.rest().starts_with(')') {
            if !cursor.eat(")") {
                return None;
            }
            return Some(Datum::List(items));
        }
        items.push(parse_value(cursor)?);
    }
}

fn symbol_from(text: String) -> Symbol {
    match text.split_once('/') {
        Some((namespace, name))
            if !namespace.is_empty() && !name.is_empty() && !name.contains('/') =>
        {
            Symbol::qualified(namespace, name)
        }
        _ => Symbol::new(text),
    }
}

fn count(value: u64) -> Datum {
    Datum::Number(NumberLiteral {
        domain: Symbol::qualified("numbers", "u64"),
        canonical: value.to_string(),
    })
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strings_round_trip_utf8_quotes_and_literal_backslashes() {
        let record = Datum::Node {
            tag: Symbol::qualified("world", "deliver-v1"),
            fields: vec![
                (
                    Symbol::new("note"),
                    Datum::String("café \\ path \"q\"".to_owned()),
                ),
                (Symbol::new("tail"), Datum::String("\\".to_owned())),
            ],
        };
        let text = export(&record).expect("export");
        let opened = open(&text).expect("revision").expect("record");
        assert_eq!(opened, record);
    }

    #[test]
    fn a_non_text_value_is_not_a_query_record() {
        let action = Datum::Node {
            tag: Symbol::qualified("world", "deliver-action-v1"),
            fields: vec![
                (Symbol::new("acknowledgement"), Datum::String("ack".into())),
                (Symbol::new("cause"), Datum::String("cause".into())),
                (Symbol::new("class"), Datum::String("class".into())),
                (Symbol::new("intent"), Datum::String("intent".into())),
                (
                    Symbol::new("selector"),
                    Datum::String("git/branch/main".into()),
                ),
                (Symbol::new("state"), Datum::String("current".into())),
                (Symbol::new("dispatches"), super::count(0)),
                (Symbol::new("observations"), super::count(0)),
                (Symbol::new("performer-calls"), super::count(0)),
                (Symbol::new("value"), super::count(0)),
            ],
        };
        let record = Datum::Node {
            tag: Symbol::qualified("world", "deliver-v1"),
            fields: vec![
                (Symbol::new("blocked"), Datum::Bool(false)),
                (Symbol::new("dispatches"), super::count(0)),
                (Symbol::new("noop"), Datum::Bool(true)),
                (Symbol::new("performer-calls"), super::count(0)),
                (Symbol::new("actions"), Datum::Vector(vec![action])),
                (Symbol::new("omitted"), Datum::Vector(vec![])),
                (Symbol::new("realization"), Datum::String("model".into())),
            ],
        };
        let text = export(&record).expect("export");
        let status = super::super::status("model", Some(&text)).expect("status");
        assert_eq!(gap_reason(&status), "refresh-needed");
        let why = super::super::why("model", "git/branch/main", Some(&text)).expect("why");
        assert_eq!(gap_reason(&why), "refresh-needed");
    }

    #[test]
    fn a_mistyped_counter_is_not_a_query_record() {
        let mut record = counted_delivery(super::count(0), super::count(0));
        let Datum::Node { fields, .. } = &mut record else {
            panic!("plan");
        };
        let dispatches = fields
            .iter_mut()
            .find(|(name, _)| name.as_qualified_str() == "dispatches")
            .expect("dispatches");
        dispatches.1 = Datum::String("oops".into());
        let text = export(&record).expect("export");
        let status = super::super::status("model", Some(&text)).expect("status");
        assert_eq!(gap_reason(&status), "refresh-needed");

        let mut record = counted_delivery(super::count(0), Datum::String("oops".into()));
        let Datum::Node { fields, .. } = &mut record else {
            panic!("plan");
        };
        let Datum::Vector(actions) = fields
            .iter_mut()
            .find(|(name, _)| name.as_qualified_str() == "actions")
            .map(|(_, value)| value)
            .expect("actions")
        else {
            panic!("actions");
        };
        let Datum::Node {
            fields: row_fields, ..
        } = &mut actions[0]
        else {
            panic!("action");
        };
        let observations = row_fields
            .iter_mut()
            .find(|(name, _)| name.as_qualified_str() == "observations")
            .expect("observations");
        observations.1 = Datum::String("oops".into());
        let text = export(&record).expect("export");
        let why = super::super::why("model", "git/branch/main", Some(&text)).expect("why");
        assert_eq!(gap_reason(&why), "refresh-needed");
    }

    fn counted_delivery(dispatches: Datum, observations: Datum) -> Datum {
        let action = Datum::Node {
            tag: Symbol::qualified("world", "deliver-action-v1"),
            fields: vec![
                (Symbol::new("acknowledgement"), Datum::String("ack".into())),
                (Symbol::new("cause"), Datum::String("cause".into())),
                (Symbol::new("class"), Datum::String("class".into())),
                (Symbol::new("intent"), Datum::String("intent".into())),
                (
                    Symbol::new("selector"),
                    Datum::String("git/branch/main".into()),
                ),
                (Symbol::new("state"), Datum::String("current".into())),
                (Symbol::new("dispatches"), dispatches),
                (Symbol::new("observations"), observations),
                (Symbol::new("performer-calls"), super::count(0)),
            ],
        };
        Datum::Node {
            tag: Symbol::qualified("world", "deliver-v1"),
            fields: vec![
                (Symbol::new("blocked"), Datum::Bool(false)),
                (Symbol::new("dispatches"), super::count(0)),
                (Symbol::new("noop"), Datum::Bool(true)),
                (Symbol::new("performer-calls"), super::count(0)),
                (Symbol::new("actions"), Datum::Vector(vec![action])),
                (Symbol::new("omitted"), Datum::Vector(vec![])),
                (Symbol::new("realization"), Datum::String("model".into())),
            ],
        }
    }

    #[test]
    fn a_gap_without_its_fields_is_not_a_query_record() {
        let record = Datum::Node {
            tag: Symbol::qualified("world", "deliver-gap-v1"),
            fields: vec![(Symbol::new("realization"), Datum::String("model".into()))],
        };
        let text = export(&record).expect("export");
        let status = super::super::status("model", Some(&text)).expect("status");
        assert_eq!(gap_reason(&status), "refresh-needed");
        let why = super::super::why("model", "git/branch/main", Some(&text)).expect("why");
        assert_eq!(gap_reason(&why), "refresh-needed");
        let Datum::Node { fields, .. } = status else {
            panic!("gap");
        };
        assert_eq!(
            fields
                .iter()
                .find(|(name, _)| name.as_qualified_str() == "resident")
                .map(|(_, value)| value),
            Some(&Datum::Bool(false))
        );
    }

    fn gap_reason(value: &Datum) -> &str {
        let Datum::Node { tag, fields } = value else {
            panic!("node, got {value:?}");
        };
        assert_eq!(tag.as_qualified_str(), "world/deliver-gap-v1");
        fields
            .iter()
            .find_map(|(name, field)| (name.as_qualified_str() == "reason").then_some(field))
            .and_then(|field| match field {
                Datum::String(text) => Some(text.as_str()),
                _ => None,
            })
            .expect("reason")
    }
}
