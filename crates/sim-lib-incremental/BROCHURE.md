# sim-lib-incremental

In one line: It loads incremental calculation into SIM as a capability-gated runtime organ for expression values.

## What it gives you

This crate wraps `sim-incremental-core` with SIM value conversion, runtime
registration, Shape descriptors, and public callables. A library can register
query functions, invalidate named keys, verify dirty roots, explain dependency
edges, snapshot graph state, and read calculation metrics without importing a
private calculation loop. The organ keeps the generic engine separate while
presenting ordinary SIM expressions at the runtime boundary.

## Why you will be glad

- Registration and invalidation are explicit runtime operations.
- Verification, explanations, snapshots, and metrics share one public surface.
- Capability checks guard mutation, observation, and management entrypoints.

## Where it fits

Use this crate when a SIM organ, language surface, or product feature needs
memoized calculation over expression values. It is the loadable runtime layer;
the core algorithm stays in `sim-incremental-core`, and domain-specific callers
add their own query families on top.
