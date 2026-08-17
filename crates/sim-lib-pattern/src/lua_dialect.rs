//! Lua text-pattern compiler for the shared VM.

use sim_kernel::{Error, Result};

use crate::{
    Anchor, CaptureId, EnginePolicy, IrNode, PatternDialect, PatternIr, RepeatBounds, ScalarDomain,
    TextClass, TextOp,
};
use std::collections::BTreeMap;

/// Lua-only operations admitted by the shared text automaton.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum LuaExtension {
    /// Match one character from a Lua character class.
    Class(TextClass),
    /// Match a delimiter pair, including nested pairs.
    Balanced {
        /// Opening delimiter.
        open: char,
        /// Closing delimiter.
        close: char,
    },
    /// Assert a transition from outside to inside a Lua character class.
    Frontier(TextClass),
}

/// Compiler for Lua-style text patterns.
#[derive(Clone, Copy, Debug, Default)]
pub struct LuaPatternDialect;

impl PatternDialect for LuaPatternDialect {
    fn compile(&self, pattern: &str) -> Result<Vec<TextOp>> {
        let ir = self.compile_ir(pattern)?;
        Ok(project_compatibility_program(ir.root()))
    }
}

impl LuaPatternDialect {
    /// Lowers Lua syntax directly into validated shared pattern IR.
    pub fn compile_ir(self, pattern: &str) -> Result<PatternIr<ScalarDomain, LuaExtension>> {
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

    fn compile(mut self) -> Result<PatternIr<ScalarDomain, LuaExtension>> {
        let mut frames = vec![Vec::new()];
        let mut next_capture = 0u32;
        while let Some(ch) = self.next() {
            match ch {
                '^' if frames.len() == 1 && frames[0].is_empty() => {
                    frames[0].push(IrNode::Anchor(Anchor::SubjectStart));
                }
                '^' => self.push_atom(&mut frames, IrNode::Symbol('^'))?,
                '$' if self.is_end() => frames
                    .last_mut()
                    .expect("root frame exists")
                    .push(IrNode::Anchor(Anchor::SubjectEnd)),
                '$' => self.push_atom(&mut frames, IrNode::Symbol('$'))?,
                '.' => self.push_atom(&mut frames, IrNode::Any)?,
                '(' => frames.push(Vec::new()),
                ')' => {
                    if frames.len() == 1 {
                        return Err(malformed("capture close without open"));
                    }
                    let body = IrNode::Concat(frames.pop().expect("capture frame exists"));
                    frames
                        .last_mut()
                        .expect("parent frame exists")
                        .push(IrNode::Capture {
                            id: CaptureId(next_capture),
                            node: Box::new(body),
                        });
                    next_capture += 1;
                }
                '[' => {
                    let set = self.parse_set()?;
                    self.push_atom(&mut frames, IrNode::Extension(LuaExtension::Class(set)))?;
                }
                '%' => {
                    let escaped = self.parse_percent()?;
                    match escaped {
                        Escaped::Atom(node) => self.push_atom(&mut frames, node)?,
                        Escaped::ZeroWidth(node) => {
                            frames.last_mut().expect("root frame exists").push(node)
                        }
                    }
                }
                '*' | '+' | '-' | '?' => return Err(malformed("quantifier without atom")),
                literal => self.push_atom(&mut frames, IrNode::Symbol(literal))?,
            }
        }
        if frames.len() != 1 {
            return Err(malformed("unterminated capture"));
        }
        let root = IrNode::Concat(frames.pop().expect("root frame exists"));
        let extensions = collect_extensions(&root);
        PatternIr::new(root, BTreeMap::new(), &EnginePolicy::new(extensions))
            .map_err(|error| malformed(&error.to_string()))
    }

    fn push_atom(
        &mut self,
        frames: &mut [Vec<IrNode<char, LuaExtension>>],
        mut node: IrNode<char, LuaExtension>,
    ) -> Result<()> {
        if let Some((min, max, greedy)) = self.peek().and_then(lua_quantifier) {
            self.index += 1;
            node = IrNode::Repeat {
                node: Box::new(node),
                bounds: RepeatBounds::new(min, max)
                    .expect("Lua quantifiers have valid static bounds"),
                greedy,
            };
        }
        frames.last_mut().expect("root frame exists").push(node);
        Ok(())
    }

    fn parse_percent(&mut self) -> Result<Escaped> {
        let Some(ch) = self.next() else {
            return Err(malformed("dangling percent escape"));
        };
        Ok(match ch {
            'a' => class_atom(TextClass::Alpha),
            'A' => class_atom(TextClass::Not(Box::new(TextClass::Alpha))),
            'd' => class_atom(TextClass::Digit),
            'D' => class_atom(TextClass::Not(Box::new(TextClass::Digit))),
            'l' => class_atom(TextClass::Lower),
            'L' => class_atom(TextClass::Not(Box::new(TextClass::Lower))),
            'u' => class_atom(TextClass::Upper),
            'U' => class_atom(TextClass::Not(Box::new(TextClass::Upper))),
            'w' => class_atom(TextClass::Alnum),
            'W' => class_atom(TextClass::Not(Box::new(TextClass::Alnum))),
            's' => class_atom(TextClass::Space),
            'S' => class_atom(TextClass::Not(Box::new(TextClass::Space))),
            'p' => class_atom(TextClass::Punct),
            'P' => class_atom(TextClass::Not(Box::new(TextClass::Punct))),
            'x' => class_atom(TextClass::Hex),
            'X' => class_atom(TextClass::Not(Box::new(TextClass::Hex))),
            'z' => class_atom(TextClass::Zero),
            'b' => {
                let open = self
                    .next()
                    .ok_or_else(|| malformed("balanced pattern missing open delimiter"))?;
                let close = self
                    .next()
                    .ok_or_else(|| malformed("balanced pattern missing close delimiter"))?;
                Escaped::Atom(IrNode::Extension(LuaExtension::Balanced { open, close }))
            }
            'f' => {
                if self.next() != Some('[') {
                    return Err(malformed("frontier pattern requires a character set"));
                }
                Escaped::ZeroWidth(IrNode::Extension(LuaExtension::Frontier(self.parse_set()?)))
            }
            literal => Escaped::Atom(IrNode::Symbol(literal)),
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
    Atom(IrNode<char, LuaExtension>),
    ZeroWidth(IrNode<char, LuaExtension>),
}

fn class_atom(class: TextClass) -> Escaped {
    Escaped::Atom(IrNode::Extension(LuaExtension::Class(class)))
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

fn lua_quantifier(ch: char) -> Option<(usize, Option<usize>, bool)> {
    match ch {
        '*' => Some((0, None, true)),
        '+' => Some((1, None, true)),
        '-' => Some((0, None, false)),
        '?' => Some((0, Some(1), true)),
        _ => None,
    }
}

fn collect_extensions(node: &IrNode<char, LuaExtension>) -> Vec<LuaExtension> {
    let mut extensions = Vec::new();
    visit(node, &mut |extension| extensions.push(extension.clone()));
    extensions
}

fn project_compatibility_program(node: &IrNode<char, LuaExtension>) -> Vec<TextOp> {
    let mut ops = Vec::new();
    project(node, &mut ops);
    ops
}

fn project(node: &IrNode<char, LuaExtension>, ops: &mut Vec<TextOp>) {
    match node {
        IrNode::Symbol(ch) => ops.push(TextOp::Literal(*ch)),
        IrNode::Any => ops.push(TextOp::Any),
        IrNode::Concat(nodes) | IrNode::Alternation(nodes) => {
            for node in nodes {
                project(node, ops);
            }
        }
        IrNode::Repeat {
            node,
            bounds,
            greedy,
        } => {
            project(node, ops);
            ops.push(TextOp::Repeat {
                min: bounds.min(),
                max: bounds.max(),
                greedy: *greedy,
            });
        }
        IrNode::Group(node) => project(node, ops),
        IrNode::Capture { node, .. } => {
            ops.push(TextOp::CaptureStart);
            project(node, ops);
            ops.push(TextOp::CaptureEnd);
        }
        IrNode::Anchor(Anchor::SubjectStart) => ops.push(TextOp::AnchorStart),
        IrNode::Anchor(Anchor::SubjectEnd) => ops.push(TextOp::AnchorEnd),
        IrNode::Extension(LuaExtension::Class(class)) => ops.push(TextOp::Class(class.clone())),
        IrNode::Extension(LuaExtension::Balanced { open, close }) => {
            ops.push(TextOp::Balanced {
                open: *open,
                close: *close,
            });
        }
        IrNode::Extension(LuaExtension::Frontier(class)) => {
            ops.push(TextOp::Frontier(class.clone()));
        }
        IrNode::Assertion(_) => unreachable!("Lua lowering does not create assertions"),
    }
}

fn visit(node: &IrNode<char, LuaExtension>, f: &mut impl FnMut(&LuaExtension)) {
    match node {
        IrNode::Concat(nodes) | IrNode::Alternation(nodes) => {
            for node in nodes {
                visit(node, f);
            }
        }
        IrNode::Repeat { node, .. } | IrNode::Group(node) | IrNode::Capture { node, .. } => {
            visit(node, f);
        }
        IrNode::Extension(extension) => f(extension),
        IrNode::Symbol(_) | IrNode::Any | IrNode::Anchor(_) | IrNode::Assertion(_) => {}
    }
}

fn malformed(message: &str) -> Error {
    Error::Eval(format!("malformed Lua pattern: {message}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_match_is_one_named_adapter_node() {
        let ir = LuaPatternDialect.compile_ir("%b()").unwrap();
        let mut extensions = Vec::new();
        visit(ir.root(), &mut |extension| {
            extensions.push(extension.clone())
        });
        assert_eq!(
            extensions,
            vec![LuaExtension::Balanced {
                open: '(',
                close: ')'
            }]
        );
    }

    #[test]
    fn capture_ids_and_compatibility_boundaries_are_frozen() {
        let ir = LuaPatternDialect.compile_ir("(%a+)%s+(%d+)").unwrap();
        let mut ids = Vec::new();
        fn collect(node: &IrNode<char, LuaExtension>, ids: &mut Vec<CaptureId>) {
            match node {
                IrNode::Concat(nodes) | IrNode::Alternation(nodes) => {
                    for node in nodes {
                        collect(node, ids);
                    }
                }
                IrNode::Repeat { node, .. } | IrNode::Group(node) => collect(node, ids),
                IrNode::Capture { id, node } => {
                    ids.push(*id);
                    collect(node, ids);
                }
                IrNode::Symbol(_)
                | IrNode::Any
                | IrNode::Anchor(_)
                | IrNode::Assertion(_)
                | IrNode::Extension(_) => {}
            }
        }
        collect(ir.root(), &mut ids);
        assert_eq!(ids, vec![CaptureId(0), CaptureId(1)]);
        assert_eq!(
            project_compatibility_program(ir.root())
                .iter()
                .filter(|op| matches!(op, TextOp::CaptureStart | TextOp::CaptureEnd))
                .count(),
            4
        );
    }
}
