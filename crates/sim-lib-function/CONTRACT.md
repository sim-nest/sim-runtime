# Shared function contract

This note freezes the boundary before implementation. Discovery found protocol
and support organs, but no concrete owner of language-neutral function plans or
instances. `sim-lib-function` is therefore a new owner which composes existing
contracts instead of copying them.

## Reuse anchors

| Concern | Exact anchor | Decision |
|---|---|---|
| Invocation | `sim-kernel/src/callable.rs::Callable` | Compose; the kernel remains a protocol boundary. |
| Runtime class | `sim-kernel/src/class.rs::ClassRef` and `ObjectCompat::class` | Reuse a caller-supplied class; do not synthesize guest classes. |
| Values | `sim-kernel/src/value.rs::Value` | Reuse at invocation and binding boundaries. |
| Browsing/checking | `sim-kernel::Shape` and `Callable::browse_*_shape` | Reuse shape references; do not invent a parameter checker. |
| Authority | `sim-kernel::Cx` capability checks | Policy receives the call context; the plan stores no powers. |
| Argument partition | `sim-lib-binding/src/call.rs::{CallSignature, BoundCall}` | Extend descriptors and delegate guest legality to policy. |
| Captured bindings | `sim-lib-binding/src/cell.rs::BindingCell` | Reuse binding cells, not copied values or a second environment graph. |
| Managed identity and edges | `sim-lib-mutation/src/managed.rs::{ManagedHandle, ManagedId, EdgeKind}` | Compose the MANAGED_2 node contract so cycles remain collectible. |
| Multiple dispatch | `sim-lib-dispatch/src/generic.rs::GenericFunction` | Optional composition only; never an ordinary-call dependency. |
| Migration proof | `sim-lib-standard-core/src/harness.rs` characterization scenarios | Reuse the established capture/compare evidence owner. |
| Index declarations | `features.toml` feature, relationship, route, specimen, and enforcement records | Author when implementation creates discoverable source anchors. |

## Field-by-field migration ledger

Every field in the four discovered guest records is classified. "Neutral" means
the shared plan/instance may own an equivalent descriptor; "policy" means the
guest retains the field or interprets its neutral descriptor.

| Guest record | Current field | Classification | Frozen treatment |
|---|---|---|---|
| `JavascriptFunction` | `kind` | language policy | JavaScript retains ordinary/arrow/class-constructor semantics and receiver policy. |
| `JavascriptFunction` | `environment` | neutral, migratable | Represent capture slots through managed binding references; do not retain a parallel environment graph. |
| `JavascriptFunction` | `constructable` | language policy | Construction legality remains JavaScript policy, not a neutral call mode decision. |
| `JavascriptFunction` | `private_names` | language policy | Private brands and name legality remain JavaScript object-model state. |
| `PythonFunction` | `params` | neutral, migratable | Convert declaration order to the shared immutable parameter plan. |
| `PythonFunction` | `body` | language policy | Keep token/lowered-body representation behind the concrete Python body policy. |
| `PythonFunction` | `captures` | neutral mechanics, language policy values | Migrate names to capture slots and managed binding references; Python owns value semantics and diagnostics. |
| `PythonFunction` | `annotations` | language policy | Python retains annotation values and browse provenance; policy may project browse shapes explicitly. |
| `LuaClosure` | `name` | neutral, migratable | Store as stable plan display identity. |
| `LuaClosure` | `env` | language policy | Lua retains lexical lookup/evaluator context; it is not duplicated in the shared instance. |
| `LuaClosure` | `params` | neutral, migratable | Convert symbols to immutable parameter descriptors. |
| `LuaClosure` | `vararg` | language policy over neutral remainder descriptor | Shared binding can partition a remainder; Lua policy creates `...` and enforces Lua behavior. |
| `LuaClosure` | `body` | language policy | Keep the `Expr` and its evaluation rules in the Lua body policy. |
| `LuaClosure` | `upvalues` | neutral, migratable | Reuse `BindingCell` capture slots and managed edges directly. |
| `IslispGeneric` | `generic` | language policy / existing owner | Keep as a thin `GenericFunction` adapter; it does not migrate into an ordinary `FunctionInstance`. |

The typed-lazy `TypeclassDictionary` is also explicitly excluded: its `class`,
`instance`, and `methods` fields describe dictionary selection, not a guest
function record. `LazyRef::{generator,cached}` owns forcing and memoization and
likewise remains typed-lazy policy.

## Ownership and delegation

`FunctionPlan` will exclusively own immutable declaration metadata: parameter
and remainder descriptors, call-mode descriptors, capture-slot metadata, and a
stable display identity. It never owns an evaluator, capabilities, mutable
bindings, a class object, or executable body state.

`FunctionInstance<B>` will own one plan, one concrete guest body/policy `B`, the
managed capture references required by that plan, and an explicitly supplied
`ClassRef`. `B` stays visible in the Rust type. Invocation adapts kernel `Args`
to binding's `BoundCall`, then delegates defaults, keywords, receiver insertion,
spread/rest semantics, evaluation, results, and diagnostics to `B`.

Captured names resolve at the binding boundary. Instances retain managed binding
cells and report their edges through the MANAGED_2 contract; they neither copy
captured values nor create a global environment/body table. Class identity is
always supplied by the installing guest profile so callable identity remains
neutral without flattening guest class behavior.

## Explicit non-goals

- No `Any`, integer body handle, host-closure erasure, or global body registry.
- No evaluator, parser, bytecode format, coroutine scheduler, prototype rules,
  language errors, defaults, keyword legality, receiver rules, or spread policy.
- No implicit `sim-lib-dispatch` dependency and no conversion of single-body
  calls into generic-function selection.
- No replacement collector, environment graph, binding cell, `Shape`, class,
  capability, kernel callable protocol, or ISLISP generic function.
- No JVM method descriptors or `invokedynamic` linkage.
