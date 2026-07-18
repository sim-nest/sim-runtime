## What this changes

<!-- One or two sentences on the change and why. -->

## Checklist

- [ ] `cargo fmt --all --check` passes
- [ ] `cargo run -p xtask -- check-local-sources` passes
- [ ] `cargo run -p xtask -- check-file-sizes` passes
- [ ] `cargo test -p sim-lib-standard-core --features native-export` passes
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] `cargo doc --workspace --no-deps` passes
- [ ] `cargo run -p xtask -- simdoc --check` passes
- [ ] `cargo run -p xtask -- repo-contract --check --repo .` passes
- [ ] `cargo run -p xtask -- validation-matrix --check --repo .` passes
- [ ] `cargo run -p xtask -- crate-catalog --check --repo .` passes
- [ ] Tests added/updated for the behavior changed
- [ ] Source and Markdown are ASCII-only
- [ ] Commits are signed off (DCO: `git commit -s`)
