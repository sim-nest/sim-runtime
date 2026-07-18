#![forbid(unsafe_code)]
//! Repository maintenance tasks for sim-runtime.

mod check_local_sources;
mod file_sizes;
mod shared_tooling;
mod simdoc;

/// Dispatches the repo-local xtask command.
pub fn run(args: Vec<String>) -> Result<(), String> {
    match args.get(1).map(String::as_str) {
        Some("simdoc") => simdoc::run(args),
        Some("check-local-sources") => check_local_sources::run(args),
        Some("check-file-sizes") => file_sizes::run(&args),
        Some("repo-contract" | "validation-matrix" | "crate-catalog") => shared_tooling::run(args),
        Some(other) => Err(format!("unknown xtask subcommand `{other}`")),
        None => Err(
            "usage: xtask <simdoc|check-local-sources|check-file-sizes|repo-contract|validation-matrix|crate-catalog>"
                .to_owned(),
        ),
    }
}
