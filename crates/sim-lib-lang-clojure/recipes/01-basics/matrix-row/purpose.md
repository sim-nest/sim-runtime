# Clojure matrix row

This recipe runs the Clojure matrix row and reads its output as evidence. Pass
counts identify source cases that lower exactly as declared, gap counts identify
explicit unsupported forms, and fail counts identify mismatches.

Exact command:

```bash
cargo test -p sim-lib-lang-clojure matrix_row
```
