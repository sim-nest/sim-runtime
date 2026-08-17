# sim-lib-mutation

In one line: It lets programs change data in place while keeping every change tracked and permitted.

## What it gives you

Most of SIM favors values that do not change, but real programs sometimes need a spot they can update. This library provides those spots -- single-value cells, small holders, growable vectors, and lookup tables -- and every write to them passes through one guarded gate. That gate checks permission and records the change, so in-place edits stay auditable instead of happening in the shadows. You get the convenience of mutable state where it genuinely helps, without giving up the ability to see what changed, when, and under whose authority.

## Why you will be glad

- You can update state directly when that is simpler than rebuilding a value.
- Every write goes through one permission check, so nothing changes unguarded.
- Changes are recorded, so surprising edits can be traced back to their source.
- Stable strong, weak, and ephemeron edge identities survive unrelated removals.
- Open role payloads describe language objects without influencing collection.

## Where it fits

The kernel defines the capability and operation contracts that decide who may do what. This crate is the concrete mutation organ built on them: cells, boxes, vectors, and tables, all gated by a standard mutate capability and publishing their operation keys as claims. Libraries and language surfaces that need changeable storage rely on it, so mutation stays consistent and accountable across the runtime.

`ManagedNode<R>` is the one reusable graph node. Its checked operations allocate
monotonic edge ids, refuse overflow and capacity violations atomically, trace in
edge-id order, and clear an edge only when its kind and expected identities still
match. `R` is language-owned evidence: collectors never inspect it.
