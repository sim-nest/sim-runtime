# sim-incremental-core

In one line: It is the small, generic calculation engine that remembers what a query read and recomputes only the parts whose evidence changed.

## What it gives you

This crate lets a runtime component register named queries, read other queries
from inside a query frame, and record external observations such as missing
names, directory listings, policy revisions, or backend epochs. The engine keeps
reverse dependency edges, invalidates dependents deterministically, and reuses
memoized values when dependency stamps and fingerprints still match.

## Why you will be glad

- Nested reads build the dependency graph from actual execution.
- Equal-priority verification runs in stable key order.
- Budgets, cycles, cancellation, and snapshots fail with typed errors.

## Where it fits

The crate is deliberately free of SIM expression, codec, Table, browser, and
product types. Runtime libraries can wrap it with domain conversion and
capability checks, while the calculation algorithm stays reusable.
