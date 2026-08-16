# Source-loading authority baseline

This baseline freezes the authority split before the common source-loading
authority value is introduced. JavaScript and Python already reuse the
`ReadEvalBroker`, `ModuleLoader`, decision ledger, module receipts, and their
characterization tests. No complete common authority owner exists: each
language constructs an authority-bearing request from a language-owned
admission value, and neither surface makes both evidence streams visible.

## Request construction inventory

The complete set of production `ModuleRequest` construction sites is:

- `crates/sim-lib-lang-javascript/src/modules.rs::JavascriptModulePolicy::load_from`
- `crates/sim-lib-lang-python/src/library_core.rs::PythonModulePolicy::load`

Both sites duplicate these seven authority fields exactly: `root_id`, `root`,
`read_policy`, `requires`, and `allow` are copied from the language admission;
`codec` is copied from the policy; `importer` is supplied by JavaScript and is
always `None` in Python. `specifier` is the requested source identity and is not
authority, but is part of the same duplicated construction.

The complete set of production `ReadEvalRequest` construction sites in this
repository is:

- `crates/sim-lib-namespace/src/module.rs::ModuleLoader::finish_load`
- `crates/sim-lib-lang-javascript/src/modules.rs::DynamicJavascript::evaluate`
- `crates/sim-lib-lang-python/src/library_core.rs::DynamicPython::admit`

All three duplicate `codec`, `read_policy`, `requires`, `allow`, and
`expected_shape`; each also constructs an `origin` and a `source`. Module loads
currently force `AnyShape`, while dynamic JavaScript and Python accept a
caller-supplied result Shape.

## Evidence visibility

`ModuleLoader::receipts` is projected by both `JavascriptModulePolicy::receipts`
and `PythonModulePolicy::receipts`. It exposes identity, generation, terminal
outcome, and failure detail for linked, cache-hit, failed, and cycle attempts.

`ReadEvalBroker` records origin, codec, expected Shape, required, requested, and
active capabilities plus the terminal decision outcome in its decision ledger.
The broker can return decisions and raw ledger events, but the brokers embedded
in `ModuleLoader`, `DynamicJavascript`, and `DynamicPython` are private and none
of those three owners projects that decision evidence. Module receipts therefore
cannot be joined to read-eval decisions through a public source-loading owner.

## Frozen behavior

The characterization surface fixes the following behavior:

- successful module linking and cache hits;
- denial when module-load or caller-required power is missing;
- denial of untrusted dynamic source policy;
- rejection of absolute paths, root escape, and cross-root relative imports;
- stable same-thread cycle detection without additional storage access;
- deterministic cached failures until explicit reload;
- reload generation increments and live-binding replacement; and
- dynamic JavaScript and Python result-Shape denial.

The shared lifecycle cases live in
`crates/sim-lib-namespace/src/module/tests.rs`. Language entry-point cases live
in `crates/sim-lib-lang-javascript/src/modules.rs` and
`crates/sim-lib-lang-python/src/library_core.rs`. This division confirms that
the mechanisms exist and are reused, but the authority request and combined
receipt/decision view still have no complete common owner.
