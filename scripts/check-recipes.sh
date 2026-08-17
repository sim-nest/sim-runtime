#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$repo_root"

# The runtime recipes are executable conformance surfaces spread across the
# workspace crates. Their owning Cargo tests are the single source of truth.
cargo test --workspace --quiet
