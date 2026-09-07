# sim-lib-operation-gate

In one line: Domain-neutral capability and exact-approval gate for effectful SIM operations.

## What it gives you

A domain-neutral gate and durable handoff for content-identified operations. Callers provide the required capability, exact semantic intent, and approval bound to that request; mismatched, stale, missing, or over-broad authority fails closed before an effect adapter runs. Canonical intent binds the target, intended result, and replay policy while grants, attempts, and bounded writer-fenced leases stay separate. The journal records dispatch before an injected performer runs and retains its raw acknowledgement. Recovery observes the postcondition independently before deciding whether anything may run. `ExactlyOnce` never repeats a durable dispatch. `Idempotent` permits another attempt only after the old lease expires and the observer proves the intended state absent. The result remains `AlreadyTrue`, `Verified`, `Diverged`, or `Uncertain`; acknowledgement loss never becomes assumed failure or success.

## Why you will be glad

- The public contract makes supported behavior, limits, and typed failures visible before integration.
- One owning crate prevents neighboring libraries from growing competing copies of the same policy.
- Deterministic records and checked tests keep adapters reviewable when implementations evolve.
- Crash coverage crosses every durable boundary around performance, including a lost raw receipt.
- Performer and observer identities are distinct and durable; changing the performer during recovery fails closed.

## Where it fits

Within SIM, sim-lib-operation-gate owns only the focused contract described above. Adjacent runtime libraries, platform adapters, codecs, and user surfaces can build around it while retaining their own policy. That boundary keeps the kernel small, avoids competing implementations, and lets this capability evolve without forcing unrelated components to change.
