# Runtime language conformance matrix

This recipe exercises `language_matrix()`, the assembly point that
registers the language rows. Row count confirms that every language surface is
registered, and row population confirms that each surface contributes pass or
declared-gap evidence.

Exact command:

```bash
cargo test -p sim-lib-lang-matrix language_matrix
```

To add a language row, define a `LanguageRow` producer in the language crate,
depend on that crate from `sim-lib-lang-matrix`, and register the row in
`language_matrix()`.
