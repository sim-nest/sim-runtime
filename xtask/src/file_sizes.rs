//! Rust source-size policy gate.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

const GENERAL_SOFT_LIMIT: usize = 500;
const GENERAL_HARD_LIMIT: usize = 700;
const ENTRYPOINT_SOFT_LIMIT: usize = 150;
const ENTRYPOINT_HARD_LIMIT: usize = 250;

pub fn run(args: &[String]) -> Result<(), String> {
    let program = args.first().map(String::as_str).unwrap_or("xtask");
    if args.len() != 2 {
        return Err(format!("usage: {program} check-file-sizes"));
    }

    let root = std::env::current_dir().map_err(|err| format!("current dir: {err}"))?;
    let summary = scan_root(&root)?;
    if summary.hard_failures == 0 {
        println!(
            "check-file-sizes: OK ({} Rust file(s), {} soft warning(s), 0 hard failure(s))",
            summary.files, summary.soft_warnings
        );
        Ok(())
    } else {
        Err(format!(
            "check-file-sizes: FAILED ({} Rust file(s), {} soft warning(s), {} hard failure(s))",
            summary.files, summary.soft_warnings, summary.hard_failures
        ))
    }
}

fn scan_root(root: &Path) -> Result<ScanSummary, String> {
    let mut paths = Vec::new();
    collect_rs_files(root, &mut paths)?;
    paths.sort();

    let mut summary = ScanSummary::default();
    for path in paths {
        let text =
            fs::read_to_string(&path).map_err(|err| format!("read {}: {err}", path.display()))?;
        let line_count = text.lines().count();
        let relative = path.strip_prefix(root).unwrap_or(&path);
        match classify(relative, line_count) {
            FileSizeStatus::Ok => {}
            FileSizeStatus::SoftWarning { limit } => {
                summary.soft_warnings += 1;
                eprintln!(
                    "warning: {} has {} lines (soft limit {})",
                    relative.display(),
                    line_count,
                    limit
                );
            }
            FileSizeStatus::HardFailure { limit } => {
                summary.hard_failures += 1;
                eprintln!(
                    "error: {} has {} lines (hard limit {})",
                    relative.display(),
                    line_count,
                    limit
                );
            }
        }
        summary.files += 1;
    }

    Ok(summary)
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    if should_skip_dir(dir) {
        return Ok(());
    }

    for entry in fs::read_dir(dir).map_err(|err| format!("read dir {}: {err}", dir.display()))? {
        let entry = entry.map_err(|err| format!("read dir entry {}: {err}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("stat {}: {err}", path.display()))?;
        if file_type.is_dir() {
            collect_rs_files(&path, out)?;
        } else if path.extension() == Some(OsStr::new("rs")) {
            out.push(path);
        }
    }

    Ok(())
}

fn should_skip_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(OsStr::to_str),
        Some(".git" | "target" | ".meta-workspace")
    )
}

fn classify(path: &Path, line_count: usize) -> FileSizeStatus {
    let limits = limits_for(path);
    if line_count > limits.hard {
        FileSizeStatus::HardFailure { limit: limits.hard }
    } else if line_count > limits.soft {
        FileSizeStatus::SoftWarning { limit: limits.soft }
    } else {
        FileSizeStatus::Ok
    }
}

fn limits_for(path: &Path) -> Limits {
    match path.file_name().and_then(OsStr::to_str) {
        Some("lib.rs" | "main.rs" | "mod.rs") => Limits {
            soft: ENTRYPOINT_SOFT_LIMIT,
            hard: ENTRYPOINT_HARD_LIMIT,
        },
        _ => Limits {
            soft: GENERAL_SOFT_LIMIT,
            hard: GENERAL_HARD_LIMIT,
        },
    }
}

#[derive(Debug, Default)]
struct ScanSummary {
    files: usize,
    soft_warnings: usize,
    hard_failures: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Limits {
    soft: usize,
    hard: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileSizeStatus {
    Ok,
    SoftWarning { limit: usize },
    HardFailure { limit: usize },
}
