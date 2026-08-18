# Checked class descriptors and inheritance

In one line: It gives SIM language-neutral classes with bounded lineage, checked member views, and cache invalidation tied to real descriptor revisions.

## What it gives you

This library turns class structure into an explicit runtime organ instead of leaving every guest language to invent its own hierarchy machinery. A descriptor names parents, members, construction policy, and revision evidence. Lineage can follow declared order or bounded C3 linearization, with typed failures for cycles, inconsistent precedence, missing nodes, and exhausted budgets. Derived member views are cached only while every parent and member revision still agrees. Managed ephemeron storage lets the collector reclaim both a class and its cached projection when the final strong root disappears. Subclass questions return evidence that callers can inspect rather than an unexplained boolean.

## Why you will be glad

- Python-style multiple inheritance and simpler declared hierarchies share one checked engine.
- Cache hits remain honest when any descriptor in the lineage changes.
- Bounded traversal prevents malformed class graphs from consuming unlimited work.
- Guest runtimes reuse one class protocol without sharing language-specific policy.

## Where it fits

This is the concrete class organ above the kernel `Class` contract and beside managed mutation and tracing. Guest libraries choose naming, visibility, dispatch, and object behavior; this crate owns reusable descriptor, lineage, and cache mechanics.
