//! Shell-glob text-pattern compiler for the shared VM.

use sim_kernel::{Error, Result};

use crate::lua_dialect::parse_set_body;
use crate::{PatternDialect, TextOp};

/// Compiler for small shell-style glob patterns.
#[derive(Clone, Copy, Debug, Default)]
pub struct GlobPatternDialect;

impl PatternDialect for GlobPatternDialect {
    fn compile(&self, pattern: &str) -> Result<Vec<TextOp>> {
        let chars = pattern.chars().collect::<Vec<_>>();
        let mut index = 0;
        let mut ops = vec![TextOp::AnchorStart];
        while let Some(ch) = chars.get(index).copied() {
            index += 1;
            match ch {
                '*' => {
                    ops.push(TextOp::Any);
                    ops.push(TextOp::Repeat {
                        min: 0,
                        max: None,
                        greedy: true,
                    });
                }
                '?' => ops.push(TextOp::Any),
                '[' => {
                    let negated = matches!(chars.get(index), Some('!') | Some('^'));
                    if negated {
                        index += 1;
                    }
                    ops.push(TextOp::Class(parse_set_body(
                        &chars,
                        &mut index,
                        negated,
                        "unterminated glob character set",
                    )?));
                }
                '\\' => {
                    let literal = chars
                        .get(index)
                        .copied()
                        .ok_or_else(|| malformed("dangling escape"))?;
                    index += 1;
                    ops.push(TextOp::Literal(literal));
                }
                literal => ops.push(TextOp::Literal(literal)),
            }
        }
        ops.push(TextOp::AnchorEnd);
        Ok(ops)
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
