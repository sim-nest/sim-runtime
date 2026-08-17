# sim-lib-class

`sim-lib-class` owns concrete, language-neutral class descriptors for SIM. The
first implementation checkpoint publishes the semantic inventory that fixes
the boundary before descriptors are implemented: kernel `Class` is the reused
protocol, Python contributes the C3 algorithm, and superficially similar object
models are explicitly excluded.

The inventory and characterization scenarios are Rust data exposed by the
crate, not narrative policy. Bounded lineage policies compute declared order or
C3 without recursion. `ClassCache` stores linearizations and derived member
views behind MANAGED_2 ephemerons, validates lineage-wide parent and member
revision stamps on every hit, and lets tracing reclaim a class and its cached
value together when the final strong root disappears.
