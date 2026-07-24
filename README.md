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
holds the standard runtime libraries that sit on top of `sim-kernel` -- default
organs, language profiles, core/control/dispatch behavior, profile evidence, and
the conformance harness.

## Crates

### Runtime substrates

- `sim-incremental-core` -- dependency-light incremental query graph with
  nested reads, reverse invalidation, memo cutoff, typed budgets, continuation
  tokens, and bounded snapshots.

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
  claims, diff, fidelity, harness, install, language profiles, the Lisp codec
  surface, polyglot/profile support, read/construct, and native export tests.

### Language profiles

Loadable language-profile libraries present familiar surface syntax over the
shared `Expr` graph and codec surfaces (Lisp is one codec, not the system
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
- `sim-lib-lang-matrix` -- the aggregate language conformance matrix.

## Features

The standard distribution and language profiles are loaded as libraries rather
than baked into the kernel. Feature-gated surfaces in this repo include the
`sim-lib-standard-core` `native-export` ABI check and the
`sim-lib-lang-prolog` `generated-coverage` conformance measurement.

Source-level rustdoc is the primary API reference for these crates.

### Rustdoc conventions

Public API documentation in `src/` follows one house style:

- Every public item opens with a one-line summary sentence, then context.
- The kernel defines the runtime contracts (`Cx`/`Registry`/`Lib`/`Linker`/
  `ExportRecord`, the `Shape` protocol, and the codec/eval-policy/control-policy
  contracts) but no concrete organ behavior; these crates supply the organs, the
  standard distribution, and the loadable language profiles. Each item is framed
  by its runtime role.
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

These commands are the standalone repo gate and match CI:

```bash
cargo fmt --all --check
cargo run -p xtask -- check-local-sources
cargo run -p xtask -- check-file-sizes
cargo test -p sim-lib-standard-core --features native-export
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo doc --workspace --no-deps
cargo run -p xtask -- simdoc --check
cargo run -p xtask -- repo-contract --check --repo .
cargo run -p xtask -- validation-matrix --check --repo .
cargo run -p xtask -- crate-catalog --check --repo .
```

## Documentation Lanes

`cargo run -p xtask -- simdoc` builds the public documentation lanes:

- API docs: `target/doc/`
- Agent cards: `docs/agents/cards.jsonl` and `docs/agents/card-index.json`
- Human docs: `docs/humans/`
- Diagram source lane marker: `docs/diagrams/src/README.md`
- Generated diagrams: `docs/diagrams/generated/`
- Split contract files: `docs/generated/`

The files written by `xtask simdoc` are generated; update crate metadata,
recipes, cards, or source rustdoc, then regenerate instead of hand-editing those
outputs.
