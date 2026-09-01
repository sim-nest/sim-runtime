# sim-lib-exec

In one line: It lets a trusted host run a specific outside process with clear permission and tight limits.

## What it gives you

Some useful work belongs outside the runtime: a formatter, a compiler, a small command-line helper, or another tool the host already trusts. This crate gives that work a narrow gate. The caller supplies a boot-trusted program reference, whole arguments, an opaque project root, and an empty-by-default sealed environment. The physical capsule resolves native resources; portable callers never receive a host path.

## Why you will be glad

- A process run is explicit about what starts and what authority allows it.
- Typed attempt truth distinguishes work that never spawned, completed work (including non-zero exit), proven cleanup after timeout or cancellation, and ambiguous post-spawn failure.
- Only a definitely not-dispatched attempt is safe to retry automatically.
- Time and output limits keep helper tools from taking over the session.

## Where it fits

The kernel carries the capability contract; this crate supplies the concrete host operation. Language libraries, table backends, build helpers, and supervised agents can use it when they need an outside process while keeping that process separate from SIM evaluation.
