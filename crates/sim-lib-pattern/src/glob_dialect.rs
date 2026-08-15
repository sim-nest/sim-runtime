//! Shell-glob text-pattern compiler for the shared VM.

use sim_kernel::{Error, Result};

use crate::lua_dialect::parse_set_body;
use crate::{
    Anchor, EnginePolicy, IrNode, PatternDialect, PatternIr, RepeatBounds, ScalarDomain, TextClass,
    TextOp,
};
use std::collections::BTreeMap;

/// Glob-only operations admitted by the shared text automaton.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GlobExtension {
    /// Match one character from a glob character class.
    Class(TextClass),
}

/// Compiler for small shell-style glob patterns.
#[derive(Clone, Copy, Debug, Default)]
pub struct GlobPatternDialect;

impl PatternDialect for GlobPatternDialect {
    fn compile(&self, pattern: &str) -> Result<Vec<TextOp>> {
        let ir = self.compile_ir(pattern)?;
        Ok(project_compatibility_program(ir.root()))
    }
}

impl GlobPatternDialect {
    /// Lowers glob syntax directly into validated shared pattern IR.
    pub fn compile_ir(self, pattern: &str) -> Result<PatternIr<ScalarDomain, GlobExtension>> {
        let chars = pattern.chars().collect::<Vec<_>>();
        let mut index = 0;
        let mut nodes = vec![IrNode::Anchor(Anchor::SubjectStart)];
        while let Some(ch) = chars.get(index).copied() {
            index += 1;
            match ch {
                '*' => {
                    nodes.push(IrNode::Repeat {
                        node: Box::new(IrNode::Any),
                        bounds: RepeatBounds::new(0, None)
                            .expect("glob star has valid static bounds"),
                        greedy: true,
                    });
                }
                '?' => nodes.push(IrNode::Any),
                '[' => {
                    let negated = matches!(chars.get(index), Some('!') | Some('^'));
                    if negated {
                        index += 1;
                    }
                    nodes.push(IrNode::Extension(GlobExtension::Class(parse_set_body(
                        &chars,
                        &mut index,
                        negated,
                        "unterminated glob character set",
                    )?)));
                }
                '\\' => {
                    let literal = chars
                        .get(index)
                        .copied()
                        .ok_or_else(|| malformed("dangling escape"))?;
                    index += 1;
                    nodes.push(IrNode::Symbol(literal));
                }
                literal => nodes.push(IrNode::Symbol(literal)),
            }
        }
        nodes.push(IrNode::Anchor(Anchor::SubjectEnd));
        let root = IrNode::Concat(nodes);
        let extensions = collect_extensions(&root);
        PatternIr::new(root, BTreeMap::new(), &EnginePolicy::new(extensions))
            .map_err(|error| malformed(&error.to_string()))
    }
}

/// Compiles a shell-style glob into shared VM operations.
///
/// # Errors
///
/// Returns an error when the glob pattern is malformed.
pub fn compile_glob_pattern(pattern: &str) -> Result<Vec<TextOp>> {
    GlobPatternDialect.compile(pattern)
}

fn malformed(message: &str) -> Error {
    Error::Eval(format!("malformed glob pattern: {message}"))
}

fn collect_extensions(node: &IrNode<char, GlobExtension>) -> Vec<GlobExtension> {
    let mut extensions = Vec::new();
    visit(node, &mut |extension| extensions.push(extension.clone()));
    extensions
}

fn project_compatibility_program(node: &IrNode<char, GlobExtension>) -> Vec<TextOp> {
    let mut ops = Vec::new();
    project(node, &mut ops);
    ops
}

fn project(node: &IrNode<char, GlobExtension>, ops: &mut Vec<TextOp>) {
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
        IrNode::Group(node) | IrNode::Capture { node, .. } => project(node, ops),
        IrNode::Anchor(Anchor::SubjectStart) => ops.push(TextOp::AnchorStart),
        IrNode::Anchor(Anchor::SubjectEnd) => ops.push(TextOp::AnchorEnd),
        IrNode::Extension(GlobExtension::Class(class)) => ops.push(TextOp::Class(class.clone())),
        IrNode::Assertion(_) => unreachable!("glob lowering does not create assertions"),
    }
}

fn visit(node: &IrNode<char, GlobExtension>, f: &mut impl FnMut(&GlobExtension)) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frozen_globs_project_from_validated_ir() {
        for pattern in ["*.rs", "src/?ain.rs", "file[!0-9].txt", r"literal\*"] {
            let ir = GlobPatternDialect.compile_ir(pattern).unwrap();
            assert_eq!(
                project_compatibility_program(ir.root()),
                compile_glob_pattern(pattern).unwrap(),
                "{pattern:?}"
            );
        }
    }
}
