# sim-incremental-core

In one line: It remembers what a calculation read and names exactly which semantic facts can affect a conclusion.

## What it gives you

This crate lets a runtime component register named queries, read other queries
from inside a query frame, and record external observations such as missing
names, directory listings, policy revisions, or backend epochs. The engine keeps
reverse dependency edges, invalidates dependents deterministically, and reuses
memoized values when dependency stamps and fingerprints still match.

It also owns open semantic projection. A loaded provider receives only a
Shape-checked immutable selection of canonical facts. Admission binds the exact
provider code and policy through either reviewed native source plus dependencies
or a closed deterministic Wasm universe. Semantic digests exclude diagnostic
envelopes, while federated owner graphs return exact invalidation and causal
paths. Explicit logical path, glob, and ignore rules reject host path aliases.

The stage-one closure assay compares D1-D6 predictions with an independently
frozen oracle. Each report states its declared-conclusion denominator, exact
affected and unaffected sets, explanation paths, semantic journal delta, and
planned starts without executing proof or claiming receipt reuse. Failed rows
emit a canonical bounded repair set; a third unchanged failure requires an
architecture review and maintainer direction before a higher rung can open.

## Why you will be glad

- Nested reads build the dependency graph from actual execution.
- Equal-priority verification runs in stable key order.
- Budgets, cycles, cancellation, and snapshots fail with typed errors.
- Missing qualification, missing confinement, and undeclared reads fail at
  distinct boundaries.
- Baseline providers cover source, build, Git, Index, release, ownership, and
  disclosure-policy facts without closing the registry.
- A passing stage-one token exists only when all six expected closures and
  their no-work, revision, and carried-state contracts pass together.

## Where it fits

The memo and dataflow engines remain generic Rust algorithms. The projection
boundary uses kernel `Datum` and `ContentId` so every loaded library shares one
canonical semantic identity. It contains no observation adapter, codec, Table,
browser, command, scheduler, or mutation behavior; products compose it from
above.
