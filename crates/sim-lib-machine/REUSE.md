# Ownership and reuse ledger

This ledger records exact inspected anchors. The new crate owns only the missing
decoded-instruction-machine policy boundary; later implementation composes the
existing organs named here.

| Need | Exact anchors | Decision | Ownership reason |
|---|---|---|---|
| Instruction execution | `sim-kernel/src/eval/protocol.rs:EvalFabric`, `sim-kernel/src/eval/policy.rs:EvalPolicy`, `sim-runtime/crates/sim-lib-exec/src/lib.rs` | new `sim-lib-machine` | Kernel evaluation describes expression evaluation and location-transparent realization. `sim-lib-exec` explicitly launches bounded host processes. Neither owns decoded instructions, machine frames, or safepoints. |
| Abrupt control | `sim-runtime/crates/sim-lib-control/src/unwind.rs:Unwind`, `sim-runtime/crates/sim-lib-control/src/close.rs:CloseStack`, `sim-runtime/crates/sim-lib-control/src/protected.rs:ProtectedCall` | compose control | Existing language-neutral unwind, cleanup, and protected-call contracts remain the owners; the machine policy only identifies handler state. |
| Resume | `sim-runtime/crates/sim-lib-control/src/resume.rs:ResumePacket`, `sim-runtime/crates/sim-lib-control/src/resume.rs:ResumableFrame` | compose control resume | Resume lifecycle and bounded one-shot completion already have a neutral owner. |
| Root enumeration | `sim-runtime/crates/sim-lib-mutation/src/managed/handles.rs:RootId`, `sim-runtime/crates/sim-lib-mutation/src/managed/handles.rs:RootedHandle`, `sim-runtime/crates/sim-lib-gc-tracing/src/collector.rs:ManagedHeap` | compose managed roots | The delivered managed arena and collector must see machine roots; a private registry would split liveness authority. |
| Work limits | `sim-runtime/crates/sim-lib-control/src/jobs.rs:WorkLimit`, `sim-runtime/crates/sim-lib-control/src/resume.rs:StepBudget` | compose control work | Charging and exhaustion semantics already exist and must not be duplicated by an instruction organ. |
| Shapes | `sim-shape/src/lib.rs:Shape`, `sim-kernel/src/object.rs:ShapeRef` | compose Shape | Admission consumes caller-owned structural policy; it does not introduce a second type or verification lattice. |
| Instruction manifests and codecs | `sim-kernel/src/codec.rs:Codec`, `sim-kernel/src/library/transaction.rs:Linker::codec` | compose codec/linker | Consumers own instruction identities and decoding. The linker registers supplied behavior but is not an execution engine. |
| Located values | `sim-kernel/src/source.rs:Span`, `sim-kernel/src/expr.rs:Expr` | compose location concepts | Stable code locations should follow the existing source-coordinate vocabulary without making expressions the instruction representation. |

## Rejected owners

`EvalPolicy` operates on SIM expressions and values, while `EvalFabric` routes a
request without exposing its local or remote realization. Moving decoded
instruction state into either would bake a concrete execution mechanism into
the kernel protocol.

`sim-lib-exec` is an authority-gated host operation with arguments, a timeout,
and captured process output. Treating it as the owner would confuse running a
host program with iterating admitted guest instructions and would import host
effects into a neutral organ.
