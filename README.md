# sim-runtime

The working machinery of a running SIM program -- names, dispatch, control, and
languages -- packaged as loadable libraries you install into a registry to give
your runtime its default organs, standard distribution, and language profiles.

New to SIM? The `sim` CLI (`cargo install sim-run`) and the sim-say walkthrough
are the front door; this repo is the library set those tools load.

## Example

```bash
cargo add sim-lib-core
```

Each library declares the manifest name it installs under. `sim-lib-core` is the
shared surface-pack substrate; asking for its name returns the qualified
`lisp:core` symbol:

```rust
use sim_kernel::Symbol;

assert_eq!(sim_lib_core::manifest_name(), Symbol::qualified("lisp", "core"));
```

(from the `manifest_name` doctest in `crates/sim-lib-core/src/lib.rs`.)

sim-runtime is a repository in the SIM constellation. SIM is an expandable Rust
runtime built around a small protocol kernel plus a large set of loadable
libraries: the kernel defines contracts, libraries provide behavior. This repo
holds the standard runtime libraries that sit on top of `sim-kernel` -- the
default organs, language profiles, core/control/dispatch behavior, and the
data-driven topology engine.

## Crates

### Core runtime organs

- `sim-lib-core` -- shared surface-pack substrate: declare exported value cards
  as data and install them once, idempotently, into a registry.
- `sim-lib-control` -- control behavior (async, backtracking, claims,
  conditions) layered over the kernel control-policy contracts.
- `sim-lib-dispatch` -- generic functions and method dispatch.
- `sim-lib-binding` -- binding behavior.
- `sim-lib-pattern` -- pattern surfaces over the kernel `Shape` protocol.
- `sim-lib-sequence` -- sequence behavior.
- `sim-lib-namespace` -- namespace behavior.
- `sim-lib-logic` -- logic behavior.
- `sim-lib-mutation` -- mutation behavior.
- `sim-lib-standard-core` -- the standard distribution core: capabilities,
  claims, diff, fidelity, harness, install, language profiles, the lisp codec
  surface, polyglot/profile support, and read/construct.

### Language profiles

Loadable language-profile libraries that present familiar surface syntax over
the shared `Expr` graph and codec surfaces (Lisp is one codec, not the system
identity):

- `sim-lib-lang-cl`, `sim-lib-lang-islisp`, `sim-lib-lang-scheme`,
  `sim-lib-lang-clojure` -- Lisp-family profiles.
- `sim-lib-lang-julia`, `sim-lib-lang-lua`, `sim-lib-lang-ruby` -- non-Lisp
  language profiles.
- `sim-lib-lang-prolog` -- Prolog surface profile backed by the logic organ,
  sequence, control, and number-tower projections.
- `sim-lib-lang-typed-lazy` -- a typed, lazy language profile.
- `sim-lib-lang-genconf` -- Shape-driven expression-space generation, codec
  registry, generated coverage reports, landmark anchors, and coverage Card
  fields for the shared language matrix.

### Topology

- `sim-lib-topology` -- the data-driven topology engine (see below).

## Topology

`sim-lib-topology` is a data-driven topology engine for SIM runtime objects.
Topologies are authored as graph data rather than code, then validated and
compiled into deterministic runtime plans.

### Authoring

The public graph model uses `Graph`, `Node`, `Edge`, `PortRef`, `Cell`,
`Budget`, and `Scheduler` values. Graph data can be authored from:

- canonical expression maps,
- friendly Lisp list forms,
- the line-oriented topology text DSL,
- ASCII diagrams, or
- section-based `.simtopo` packages (`graph:`, `tests:`, `capabilities:`,
  ...), parsed by `parse_package` / `load_package_file` into a
  `TopologyPackage`.

### Validation and compile plans

`compile_graph` validates a graph and lowers it to a `CompiledGraph` with stable
node and edge indexes. Static validation covers duplicate nodes, bad endpoints,
missing required ports, reachability, bounded cycles, shape expressions,
capabilities, and budgets. Compile plans are deterministic, so the same graph
always yields the same stable plan.

### Runtime execution

Core node verbs cover input, output, calls, wiring, cells, branches, loops,
fanout, merge, race, quorum, reduce, spawn, and patch operations. A
`TopologySite` server connection surface (`connection_from_graph`) exposes a
topology as a live endpoint, and `Budget`/`Scheduler` values bound execution.

### Adapters

`TopologyAdapter` / `TopologyAdapterRegistry` bind topology nodes to runtime
objects: server connections, callables, shapes, codecs, agents, streams,
tables, lists, and nested topologies. DAW session launch packages are produced
on the audio side (`daw_session_topology_package` in the `sim-audio-daw` repo,
which emits a `TopologyPackage`) and run through this same adapter surface.

### Reflection, replay, and patching

The engine supports reflection and explanation (`topology_reflect`,
`topology_explain`, `TopologyRunReport`), replay and counterfactual replay
(`replay_report`, `counterfactual_replay`), and live patching
(`TopologyPatch`, `apply_topology_patch`, `PatchOp`).

### Registry, Cards, and tests

A topology registry (`TopologyRegistry`, `install_topology_lib`,
`topology_load_file`, `topology_reload`) manages loaded topologies. Browse and
help Cards (`topology_card_expr`, `topology_browse_symbols`,
`topology_verb_specs`, `topology_function_specs`, `topology_example_specs`) make
topologies discoverable, and `.simtopo` packages carry their own `GraphTest`
cases so package tests run as generated examples.

## Feature families

Relevant root feature families include `topology-core` and `topology`. The
language-profile and standard-distribution libraries are loaded as libs by
default rather than baked into the kernel.

Source-level rustdoc is the primary API reference for these crates.

### Rustdoc conventions

Public API documentation in `src/` follows one house style:

- Every public item opens with a one-line summary sentence, then context.
- The kernel defines the runtime contracts (`Cx`/`Registry`/`Lib`/`Linker`/
  `ExportRecord`, the `Shape` protocol, and the codec/eval-policy/control-policy
  contracts) but no concrete organ behavior; these crates supply the organs, the
  standard distribution, the topology engine, and the loadable language profiles.
  Each item is framed by its runtime role.
- The first-reach types carry a `# Examples` doctest that compiles and passes.
- Cross-reference with intra-doc links, and link back to this README rather than
  restating it.

The public API is documentation-gated: each crate's `lib.rs` denies
`missing_docs`, so every public item, field, and variant must be documented for
the crate to build.

Each crate's runnable examples are its embedded `recipes/` tree plus the rustdoc
`# Examples` doctests; there are no stub recipe directories. Recipes are codec
source with a `requires` library list and are exercised as generated examples.

## Validation

These commands run in the constellation workspace; only `sim-kernel` builds
from a lone clone today (see `DEVELOPING.md` in `sim-sdk`).

```bash
cargo fmt --check && cargo test --workspace && cargo clippy --workspace -- -D warnings && cargo doc --workspace --no-deps
cargo run -p xtask -- simdoc --check
```

## Documentation Lanes

`cargo run -p xtask -- simdoc` builds the public documentation lanes:

- API docs: `target/doc/`
- Agent cards: `docs/agents/cards.jsonl` and `docs/agents/card-index.json`
- Human docs: `docs/humans/`
- Diagrams: `docs/diagrams/src/` and `docs/diagrams/generated/`

The same command writes split contract files under `docs/generated/`. Everything
under `docs/` is generated; do not hand-edit it.
