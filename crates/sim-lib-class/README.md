# sim-lib-class

`sim-lib-class` owns concrete, language-neutral class descriptors for SIM. The
first implementation checkpoint publishes the semantic inventory that fixes
the boundary before descriptors are implemented: kernel `Class` is the reused
protocol, Python contributes the C3 algorithm, and superficially similar object
models are explicitly excluded.

The inventory and characterization scenarios are Rust data exposed by the
crate, not narrative policy. Later implementation guards can therefore reject
semantic drift directly.
