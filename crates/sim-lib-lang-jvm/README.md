# sim-lib-lang-jvm

`sim-lib-lang-jvm` is the loadable JVM profile for SIM. Its first checkpoint is
intentionally semantics-free: `reuse-ledger.toml`, `supported-runtime.toml`, and
`intrinsics.toml` freeze the owners and admitted boundary before execution code
is introduced.

The JVM consumes decoded classfiles from `sim-codec-classfile`, exact code-unit
text from `sim-text`, the neutral bounded machine, the class and managed-object
organs, authorized module sources, and the shared `Raised` envelope. It owns no
classfile parser, opcode inventory, ambient classpath, or private unwind carrier.

Ordinary SIM code reaches the profile through the shaped `jvm/define`,
`jvm/invoke-static`, `jvm/invoke-instance`, `jvm/browse`, `jvm/profile`, and
`jvm/fidelity` callables. Definition accepts only explicitly supplied bytes;
invocation and bounded browsing require separate capabilities. Fidelity always
names the absent verifier, class library, and lambda linkage before positive
evidence.
