# JavaScript JSON and collection characterization baseline

The machine-readable baseline is
`fixtures/compose3-json-collections.toml`. Every scenario records a canonical
outcome and a SHA-256 content identity over its domain, scenario name, source
anchor, and outcome, separated by U+001F. Recomputing the identities twice must
produce identical bytes.

This is deliberately a characterization, not an ECMAScript wish list. In
particular, the current JSON value enum cannot construct a reference cycle, so
the declared `Cycle` error and ancestor check have no reachable cyclic input.
Arrays can grow by indexed assignment and shrink one cell through `pop`, but
there is no general length setter. Array, set, and explicit iterator values are
owned snapshots, so mutations after iterator construction are not observed.
Map entry iteration borrows the map and therefore cannot overlap mutation.

The baseline also preserves asymmetries that later composition must compare
rather than silently normalize: `map` checks its complete visit count before
calling back, while `filter` may call back before reporting `Limit`; array
iteration materializes holes as `Undefined`; deletion followed by reinsertion
moves a Map or Set value to the end; and symbol descriptions do not contribute
to identity.

The codec-side reuse ledger is
`sim-codecs/docs/compose3-json-reuse-ledger.md`. It names the exact projection
functions already called by the guest and records which retired COMPOSE1 claim
the current source contradicts.
