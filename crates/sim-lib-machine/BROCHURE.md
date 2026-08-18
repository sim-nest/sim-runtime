# Neutral instruction-machine contracts

In one line: It gives guest runtimes one bounded instruction-machine skeleton while leaving opcode meaning and language policy to the guest.

## What it gives you

This library defines the reusable mechanics of a decoded-instruction machine without baking in a particular bytecode. A guest supplies instruction and value types, logical width rules, frame behavior, handler lookup, and effect policy. The shared driver accounts for work, advances frames, records deterministic receipts, exposes safepoints, and keeps managed roots visible while execution is in flight. Effect-free preparation can be admitted separately from effectful work, so verification and optimization do not silently acquire runtime powers. Typed outcomes distinguish completion, suspension, raised values, budget exhaustion, and policy refusal.

## Why you will be glad

- New guest runtimes reuse bounded frames and work accounting instead of cloning an interpreter loop.
- Safepoints and managed roots remain explicit for collectors and interruption.
- Deterministic receipts make execution decisions inspectable and replay-friendly.
- Guest-specific opcodes, handlers, and effects remain replaceable policy.

## Where it fits

This is the neutral machine organ between decoded guest instructions and a loadable language runtime. JVM and other profiles provide semantics and capabilities; the kernel carries generic values, control, and managed-object contracts without learning any guest instruction set.
