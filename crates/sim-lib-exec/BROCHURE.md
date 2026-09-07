# sim-lib-exec

In one line: It lets a trusted host run a specific outside process with clear permission and tight limits.

## What it gives you

Some useful work belongs outside the runtime: a formatter, a compiler, a small command-line helper, or another tool the host already trusts. This crate gives that work a narrow gate. The caller supplies a boot-trusted program reference, whole arguments, an opaque project root, and an empty-by-default sealed environment. The physical capsule resolves native resources; portable callers never receive a host path.

For implementation checks, an immutable `CommandSpec` binds the executable or interpreter, unchanged script bytes, cwd, environment, source and writable resources, timeout, output contract, network authority, and cleanup policy into one `CommandId`. A `LocalCheckRequest` names an installed id instead of carrying mutable command text, and `LocalCheckPort` is the portable seam used by packet tooling.

## Why you will be glad

- A process run is explicit about what starts and what authority allows it.
- Typed attempt truth distinguishes work that never spawned, completed work (including non-zero exit), proven cleanup after timeout or cancellation, and ambiguous post-spawn failure.
- Only a definitely not-dispatched attempt is safe to retry automatically.
- Time and output limits keep helper tools from taking over the session.
- Compound manifest commands preserve their exact trusted script bytes; no caller slot can become shell interpolation.
- Network access needs its own matching capability grant, while a networkless sandbox requires positive isolation evidence.

## Where it fits

The kernel carries the capability contract; this crate supplies the concrete host operation. Language libraries, table backends, build helpers, and supervised agents can use it when they need an outside process while keeping that process separate from SIM evaluation.
