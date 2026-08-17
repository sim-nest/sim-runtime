# Managed graph migration baseline

This note freezes the pre-migration managed-graph seams. It is an inventory,
not a second contract: the Rust APIs named below remain authoritative.

## Existing owners and dependency direction

- `sim-lib-mutation` owns `ManagedArena<T>`, stable allocation/root handles,
  `TraceContractVersion::V1`, `ManagedObject`, `EdgeVisitor`, safepoints,
  mutation epochs, atomic clear/sweep application, and clearing receipts.
- `sim-lib-gc-tracing` depends on `sim-lib-mutation`. It owns `collect`, the
  bounded read-only collection plan, ephemeron fixpoint, atomic plan
  application, and `CollectionReceipt` ordering.
- `sim-lib-lang-javascript` and `sim-lib-lang-python` depend on both owners.
  Neither shared owner depends on a guest. A reusable heap wrapper that calls
  `collect` must therefore live in `sim-lib-gc-tracing`; placing it in mutation
  would reverse this edge and create a cycle.

The arena retains allocations by default under `HardCappedRetainPolicy`.
Tracing is explicitly selected by a wrapper and is not an arena policy.

## Frozen trace and clear ABI

`ManagedObject::trace_edges` enumerates `(EdgeId, ManagedId)` strong and weak
edges and `(EdgeId, key, value)` ephemerons through `EdgeVisitor`. Conditional
clears receive the edge identity and expected target identities and report
whether that invocation performed the clear. `apply_collection_at_epoch`
checks the captured mutation epoch, clears weak entries, clears ephemerons,
then sweeps, returning each category in that order. `CollectionReceipt`
preserves allocation order for marked and swept identities and visitation order
for clear receipts.

The replacement node must keep stable edge identities. The guest forks below
currently derive `EdgeId` from `Vec` position, so removing an entry renumbers
later evidence and cannot be preserved as the future mutation API.

## Exact guest forks

The JavaScript and Python `managed.rs` modules duplicate the same mechanics:

1. A language role enum plus a payload containing `pub edges: Vec<ManagedId>`.
2. A `ManagedObject` implementation that enumerates the vector as strong edges
   using `EdgeId(i as u32)` and returns `false` from both clear methods.
3. A heap containing `ManagedArena<GuestManagedObject>` and a two-case
   `Tracing(CollectionLimits)` / `Retain` policy.
4. `standard` and `retaining` constructors, direct arena allocation, live
   length and policy inspection, an explicit retain-mode cycle gap, and a
   synchronous `collect` switch.
5. A `connect` method that mutates the payload directly with
   `arena.get_mut(from)?.edges.push(to.id())`.

The names and role variants differ; the payload, trait body, wrapper behavior,
and dependency shape do not.

## Migration sites requiring checked replacement

The complete direct managed-edge field-mutation inventory is:

- `crates/sim-lib-lang-javascript/src/managed.rs`:
  `JavascriptHeap::connect` pushes into `JavascriptManagedObject::edges`.
- `crates/sim-lib-lang-python/src/managed.rs`:
  `PythonHeap::connect` pushes into `PythonManagedObject::edges`.
- The JavaScript cycle test calls `connect` twice.
- The Python heterogeneous-cycle test calls `connect` for four adjacent pairs
  and once for the closing edge.

No other production module mutates either guest payload. Construction with an
empty `edges` vector in the Python test is initialization, but must migrate to
the shared node constructor when that field disappears. The replacement API
must make insertion, replacement, and removal checked and atomic; callers must
not regain mutable collection fields through a generic `get_mut` escape.

## Characterization floor

Existing mutation and collector tests are the behavioral floor: cycles and
shared reachability, root registration/churn, explicit retain mode, stale
handles, every collection limit, mutation-epoch refusal without partial
mutation, deterministic repeated runs, and weak/ephemeron/sweep receipt order.
The JavaScript and Python tests additionally freeze guest allocation,
collection, role heterogeneity, and retain-mode behavior until their migrations
replace these forks.
