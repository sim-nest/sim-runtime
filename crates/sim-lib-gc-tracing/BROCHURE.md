# sim-lib-gc-tracing

In one line: It reclaims unreachable managed-object cycles with deterministic, explicitly bounded tracing work.

## What it gives you

The crate supplies stop-the-world collection policy for `sim-lib-mutation`'s language-neutral managed arena. Marking is iterative rather than Rust-stack recursive, strong edges and ephemerons are handled in distinct phases, and object, edge, mark-stack, and total-work limits are admitted before sweeping begins. A refused collection produces a failure receipt before any destructive mutation. Successful receipts follow allocation order, making collection outcomes suitable for replay comparison and operational diagnosis.

## Why you will be glad

- Cycles can be reclaimed without teaching the kernel or language profiles about a concrete collector.
- Explicit budgets turn pathological graphs into named refusal rather than unbounded pause time.
- Deterministic traversal and receipts make tests, replay, and incident reports comparable.
- The collector scans only the managed arena contract; it never guesses through arbitrary Rust objects.

## Where it fits

Use this loadable policy when a language profile or host selects tracing collection for managed objects. Mutation and arena ownership remain in `sim-lib-mutation`; language object semantics remain in their profiles; the kernel gains no garbage-collector API or heap implementation.
