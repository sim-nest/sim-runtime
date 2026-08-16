# JVM prepared-execution evidence

The JVM profile prepares the shared classfile decoder's instruction stream once
into `LocatedCode<PreparedJvmPolicy>`. `PreparedMicroOp::Verified` exists only
when an exact whole-class proof matches the class, revision, verifier policy,
structural input, method, and converged frame. The checked path remains the
fallback. Generated `PREPARED_DISPATCH` covers all 256 opcode bytes from the
shared manifest; the bounded `FUSED_DEFINITIONS` table maps every fusion back to
its original instruction ids, source locations, work charges, and root effects.

`performance-coverage.toml` is the compact browse face. It names every source
anchor and both canonical BENCH_2 reports. Those reports live in sim-tooling,
the statistics owner, under `benchmarks/bytecode-speed-4/`. Each retains 20 raw
baseline and 20 raw candidate duration samples plus counter samples. Their
content keys are recorded in the coverage manifest, so a summary cannot be
silently detached from its raw samples.

Both frozen comparisons are explicitly `inconclusive`; this publication makes
no speedup claim. It claims only attributable execution structure and semantic
fidelity: decoded inputs are prepared before the single shared-machine drive,
specialization requires exact proof, and every admitted fusion preserves its
unfused maps. `tests/published_performance.rs` checks these statements and the
ownership guard supplies one rejected fixture for each forbidden fork.
