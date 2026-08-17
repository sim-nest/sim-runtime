# sim-lib-machine

`sim-lib-machine` defines the policy boundary for bounded decoded-instruction
machines. It deliberately contains contracts only: consumers supply instruction
identity, values, effects, frames, handlers, managed roots, safepoints,
admission, and receipts.

`ManagedRootSource` projects slots, operands, suspended frames, continuations,
and policy state into `sim-lib-mutation`'s shared managed identities.
`Driver::drive_with_safepoints` publishes a complete bounded root snapshot before
each prepared safepoint while leaving reclamation policy to the observer.

The crate owns neither a guest language nor host execution. Its public surface
excludes language object models, host scheduling, time, ambient input/output,
and text representations. Later phases build storage and drivers behind these
policy seams without weakening that boundary.

See [REUSE.md](REUSE.md) for the source-level ownership and reuse ledger.
