//! Strict portable parsing for an explicitly selected ordinary operator command.

use crate::{ArgAtom, SealedBindings};
use sim_kernel::{Error, Result};
use std::collections::BTreeMap;

/// Explicit ordinary-process selection; native path admission belongs to the host.
/// This data neither grants execution nor requests sandbox/checker qualification.
pub struct OperatorExecSelection {
    /// Exact native program locator, interpreted only by the capsule.
    pub program: String,
    /// Exact native project locator, interpreted only by the capsule.
    pub root: String,
    /// Whole child arguments following the mandatory `--` delimiter.
    pub argv: Vec<String>,
    /// Mandatory nonzero timeout.
    pub timeout_ms: u64,
    /// Mandatory nonzero shared output cap.
    pub max_output_bytes: usize,
    /// Explicit bindings; ambient environment is never inherited.
    pub environment: SealedBindings,
}

impl OperatorExecSelection {
    /// Parses words after `operator-exec`; duplicate/unknown fields fail closed.
    pub fn parse(words: &[String]) -> Result<Self> {
        let mut fields = BTreeMap::new();
        let mut environment = Vec::new();
        let mut offset = 0;
        while let Some(word) = words.get(offset) {
            if word == "--" {
                break;
            }
            if !matches!(
                word.as_str(),
                "--program" | "--root" | "--timeout-ms" | "--max-output-bytes" | "--env"
            ) {
                return Err(invalid("unknown option"));
            }
            let value = words
                .get(offset + 1)
                .ok_or_else(|| invalid("missing option value"))?;
            if word == "--env" {
                let (key, value) = value
                    .split_once('=')
                    .ok_or_else(|| invalid("binding requires NAME=VALUE"))?;
                environment.push((key.to_owned(), value.to_owned()));
            } else if fields.insert(word.as_str(), value.clone()).is_some() {
                return Err(invalid("duplicate option"));
            }
            offset += 2;
        }
        if words.get(offset).map(String::as_str) != Some("--") {
            return Err(invalid("requires -- before child argv"));
        }
        let mut required = |name| {
            fields
                .remove(name)
                .ok_or_else(|| invalid("missing required option"))
        };
        let program = required("--program")?;
        let root = required("--root")?;
        let timeout_ms = required("--timeout-ms")?
            .parse::<u64>()
            .map_err(|_| invalid("invalid timeout"))?;
        let max_output_bytes = required("--max-output-bytes")?
            .parse::<usize>()
            .map_err(|_| invalid("invalid output cap"))?;
        if timeout_ms == 0 || max_output_bytes == 0 {
            return Err(invalid("budgets must be nonzero"));
        }
        let argv = words[offset + 1..].to_vec();
        for word in &argv {
            ArgAtom::new(word.clone())?;
        }
        Ok(Self {
            program,
            root,
            argv,
            timeout_ms,
            max_output_bytes,
            environment: SealedBindings::literals(environment)?,
        })
    }
}

fn invalid(detail: &str) -> Error {
    Error::Eval(format!("operator exec: {detail}"))
}
