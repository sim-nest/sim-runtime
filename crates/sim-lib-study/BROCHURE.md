# sim-lib-study

In one line: Domain-neutral durable study lifecycle over sealed coordinates.

## What it gives you

Seal an exact experiment matrix once, resume it after failure, and inject any effect owner through one object-safe executor. Fenced claims, closure revalidation, content-bound replies, explicit retries, and replay-derived projections prevent duplicate samples and accidental reinterpretation. The contract keeps inputs, outputs, limits, and refusal cases explicit, so callers can compose the capability without acquiring unrelated host, transport, or product authority. Stable records make the result suitable for tests, inspection, and deterministic integration.

## Why you will be glad

- The public contract makes supported behavior, limits, and typed failures visible before integration.
- One owning crate prevents neighboring libraries from growing competing copies of the same policy.
- Deterministic records and checked tests keep adapters reviewable when implementations evolve.

## Where it fits

Within SIM, sim-lib-study owns only the focused contract described above. Adjacent runtime libraries, platform adapters, codecs, and user surfaces can build around it while retaining their own policy. That boundary keeps the kernel small, avoids competing implementations, and lets this capability evolve without forcing unrelated components to change.
