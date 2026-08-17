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

Sealed verification evidence is inspectable through bounded, immutable class,
method, and frame views. `VerificationExplanation` turns typed refusals into a
stable code and a human reason, while generated `VERIFIER_COVERAGE` proves that
all 256 shared opcode rows have exactly one verifier-rule owner. These views do
not accept a `verified` boolean and cannot construct or mutate a proof.

## Lambda linkage

The frozen `javac --release 8` corpus covers capturing and non-capturing
lambdas, static, bound, unbound, interface, and constructor method references,
and the serializable alternate metafactory. `fixtures/build-fixtures.py` is the
single reproducible producer; `fixtures/lambda-fixtures.toml` records compiler,
source, classfile, nested-interface hashes, and the expected bootstrap sites.

Use the recipes for Java-to-SIM lambda projection, method references, and the
SIM-to-Java functional-interface adapter. Linkage remains a class-linking
operation: it generates loader-local managed metadata without classfile bytes,
then invokes through the existing method pipeline. It never links at method
entry, stores a host closure, ranks Java overloads with Shape, introduces a
second function organ, or retains a strong global cache.
