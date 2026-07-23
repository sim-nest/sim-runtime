# sim-lib-incremental

`sim-lib-incremental` is the loadable incremental query organ for SIM runtime
expressions. It wraps `sim-incremental-core` with capability-gated SIM
functions for registering query expressions, invalidating observed keys,
verifying roots, explaining memo state, exporting snapshots, and reading
metrics.

The generic core remains independent of SIM values. This crate owns the
translation between SIM `Expr` values and the core query callbacks, plus the
organ claims and browseable shape contracts exposed by the runtime library.
