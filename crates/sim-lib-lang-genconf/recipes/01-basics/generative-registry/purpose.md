# Generative conformance registry

This recipe exercises `generative_registry()`, the registry that connects each
language row to its codec and shared `ExprSpace`. The closure test confirms
that every registered language produces a generated coverage report and that an
unanchored report exposes no coverage percentage.

Exact command:

```bash
cargo test -p sim-lib-lang-genconf closure
```

To add a language to generated coverage, add one `GenerativeRow` entry with the
language symbol, codec symbol, and `ExprSpace`. The curated conformance matrix
continues to live in `language_matrix()`.
