# sim-lib-gc-tracing

In one line: Reclaim unreachable managed-object cycles with deterministic, explicitly bounded work.

## What it gives you

- Iterative strong-edge marking and ephemeron closure without Rust-stack recursion.
- Separate object, edge, mark-stack, and total-work admission limits.
- Failure receipts produced before any sweep mutation.
- Allocation-ordered collection receipts suitable for replay comparison.

## Where it fits

The crate supplies collection policy over `sim-lib-mutation`'s language-neutral
managed arena. It never scans arbitrary Rust objects and adds nothing to the kernel.
