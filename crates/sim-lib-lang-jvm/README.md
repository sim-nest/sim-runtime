# sim-lib-lang-jvm

`sim-lib-lang-jvm` is the loadable JVM profile for SIM. Its first checkpoint is
intentionally semantics-free: `reuse-ledger.toml`, `supported-runtime.toml`, and
`intrinsics.toml` freeze the owners and admitted boundary before execution code
is introduced.

The JVM consumes decoded classfiles from `sim-codec-classfile`, exact code-unit
text from `sim-text`, the neutral bounded machine, the class and managed-object
organs, authorized module sources, and the shared `Raised` envelope. It owns no
classfile parser, opcode inventory, ambient classpath, or private unwind carrier.
