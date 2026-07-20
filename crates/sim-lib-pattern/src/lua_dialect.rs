//! Lua text-pattern compiler for the shared VM.

use sim_kernel::{Error, Result};

use crate::{PatternDialect, TextClass, TextOp};

/// Compiler for Lua-style text patterns.
#[derive(Clone, Copy, Debug, Default)]
pub struct LuaPatternDialect;

impl PatternDialect for LuaPatternDialect {
    fn compile(&self, pattern: &str) -> Result<Vec<TextOp>> {
        LuaCompiler::new(pattern).compile()
    }
}

/// Compiles a Lua-style text pattern into shared VM operations.
///
/// # Errors
///
/// Returns an error when the pattern is malformed.
pub fn compile_lua_pattern(pattern: &str) -> Result<Vec<TextOp>> {
    LuaPatternDialect.compile(pattern)
}

struct LuaCompiler {
    chars: Vec<char>,
    index: usize,
}

impl LuaCompiler {
    fn new(pattern: &str) -> Self {
        Self {
            chars: pattern.chars().collect(),
            index: 0,
        }
    }

    fn compile(mut self) -> Result<Vec<TextOp>> {
        let mut ops = Vec::new();
        while let Some(ch) = self.next() {
            match ch {
                '^' if ops.is_empty() => ops.push(TextOp::AnchorStart),
                '^' => self.push_atom(&mut ops, TextOp::Literal('^'))?,
                '$' if self.is_end() => ops.push(TextOp::AnchorEnd),
                '$' => self.push_atom(&mut ops, TextOp::Literal('$'))?,
                '.' => self.push_atom(&mut ops, TextOp::Any)?,
                '(' => ops.push(TextOp::CaptureStart),
                ')' => ops.push(TextOp::CaptureEnd),
                '[' => {
                    let set = self.parse_set()?;
                    self.push_atom(&mut ops, TextOp::Class(set))?;
                }
                '%' => {
                    let escaped = self.parse_percent()?;
                    match escaped {
                        Escaped::Atom(op) => self.push_atom(&mut ops, op)?,
                        Escaped::ZeroWidth(op) => ops.push(op),
                    }
                }
                '*' | '+' | '-' | '?' => return Err(malformed("quantifier without atom")),
                literal => self.push_atom(&mut ops, TextOp::Literal(literal))?,
            }
        }
        Ok(ops)
    }

    fn push_atom(&mut self, ops: &mut Vec<TextOp>, op: TextOp) -> Result<()> {
        ops.push(op);
        if let Some(quantifier) = self.peek().and_then(lua_quantifier) {
            self.index += 1;
            ops.push(quantifier);
        }
        Ok(())
    }

    fn parse_percent(&mut self) -> Result<Escaped> {
        let Some(ch) = self.next() else {
            return Err(malformed("dangling percent escape"));
        };
        Ok(match ch {
            'a' => Escaped::Atom(TextOp::Class(TextClass::Alpha)),
            'A' => Escaped::Atom(TextOp::Class(TextClass::Not(Box::new(TextClass::Alpha)))),
            'd' => Escaped::Atom(TextOp::Class(TextClass::Digit)),
            'D' => Escaped::Atom(TextOp::Class(TextClass::Not(Box::new(TextClass::Digit)))),
            'l' => Escaped::Atom(TextOp::Class(TextClass::Lower)),
            'L' => Escaped::Atom(TextOp::Class(TextClass::Not(Box::new(TextClass::Lower)))),
            'u' => Escaped::Atom(TextOp::Class(TextClass::Upper)),
            'U' => Escaped::Atom(TextOp::Class(TextClass::Not(Box::new(TextClass::Upper)))),
            'w' => Escaped::Atom(TextOp::Class(TextClass::Alnum)),
            'W' => Escaped::Atom(TextOp::Class(TextClass::Not(Box::new(TextClass::Alnum)))),
            's' => Escaped::Atom(TextOp::Class(TextClass::Space)),
            'S' => Escaped::Atom(TextOp::Class(TextClass::Not(Box::new(TextClass::Space)))),
            'p' => Escaped::Atom(TextOp::Class(TextClass::Punct)),
            'P' => Escaped::Atom(TextOp::Class(TextClass::Not(Box::new(TextClass::Punct)))),
            'x' => Escaped::Atom(TextOp::Class(TextClass::Hex)),
            'X' => Escaped::Atom(TextOp::Class(TextClass::Not(Box::new(TextClass::Hex)))),
            'z' => Escaped::Atom(TextOp::Class(TextClass::Zero)),
            'b' => {
                let open = self
                    .next()
                    .ok_or_else(|| malformed("balanced pattern missing open delimiter"))?;
                let close = self
                    .next()
                    .ok_or_else(|| malformed("balanced pattern missing close delimiter"))?;
                Escaped::Atom(TextOp::Balanced { open, close })
            }
            'f' => {
                if self.next() != Some('[') {
                    return Err(malformed("frontier pattern requires a character set"));
                }
                Escaped::ZeroWidth(TextOp::Frontier(self.parse_set()?))
            }
            literal => Escaped::Atom(TextOp::Literal(literal)),
        })
    }

    fn parse_set(&mut self) -> Result<TextClass> {
        let mut negated = false;
        if self.peek() == Some('^') {
            self.index += 1;
            negated = true;
        }
        parse_set_body(
            &self.chars,
            &mut self.index,
            negated,
            "unterminated character set",
        )
    }

    fn next(&mut self) -> Option<char> {
        let ch = self.chars.get(self.index).copied()?;
        self.index += 1;
        Some(ch)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.index).copied()
    }

    fn is_end(&self) -> bool {
        self.index >= self.chars.len()
    }
}

enum Escaped {
    Atom(TextOp),
    ZeroWidth(TextOp),
}

pub(crate) fn parse_set_body(
    chars: &[char],
    index: &mut usize,
    negated: bool,
    unterminated: &str,
) -> Result<TextClass> {
    let mut literals = Vec::new();
    let mut ranges = Vec::new();
    let mut classes = Vec::new();
    let mut first = true;
    while let Some(ch) = chars.get(*index).copied() {
        *index += 1;
        if ch == ']' && !first {
            return Ok(TextClass::Set {
                chars: literals,
                ranges,
                classes,
                negated,
            });
        }
        first = false;
        let item = if ch == '%' {
            let escaped = chars
                .get(*index)
                .copied()
                .ok_or_else(|| malformed("dangling set escape"))?;
            *index += 1;
            set_escape(escaped)
        } else {
            SetItem::Literal(ch)
        };
        if let SetItem::Literal(start) = item {
            if chars.get(*index).copied() == Some('-')
                && chars.get(*index + 1).is_some_and(|end| *end != ']')
            {
                *index += 1;
                let end = chars
                    .get(*index)
                    .copied()
                    .ok_or_else(|| malformed(unterminated))?;
                *index += 1;
                ranges.push((start, end));
            } else {
                literals.push(start);
            }
        } else if let SetItem::Class(class) = item {
            classes.push(class);
        }
    }
    Err(malformed(unterminated))
}

enum SetItem {
    Literal(char),
    Class(TextClass),
}

fn set_escape(ch: char) -> SetItem {
    match ch {
        'a' => SetItem::Class(TextClass::Alpha),
        'd' => SetItem::Class(TextClass::Digit),
        'l' => SetItem::Class(TextClass::Lower),
        'u' => SetItem::Class(TextClass::Upper),
        'w' => SetItem::Class(TextClass::Alnum),
        's' => SetItem::Class(TextClass::Space),
        'p' => SetItem::Class(TextClass::Punct),
        'x' => SetItem::Class(TextClass::Hex),
        'z' => SetItem::Class(TextClass::Zero),
        literal => SetItem::Literal(literal),
    }
}

fn lua_quantifier(ch: char) -> Option<TextOp> {
    match ch {
        '*' => Some(TextOp::Repeat {
            min: 0,
            max: None,
            greedy: true,
        }),
        '+' => Some(TextOp::Repeat {
            min: 1,
            max: None,
            greedy: true,
        }),
        '-' => Some(TextOp::Repeat {
            min: 0,
            max: None,
            greedy: false,
        }),
        '?' => Some(TextOp::Repeat {
            min: 0,
            max: Some(1),
            greedy: true,
        }),
        _ => None,
    }
}

fn malformed(message: &str) -> Error {
    Error::Eval(format!("malformed Lua pattern: {message}"))
}
