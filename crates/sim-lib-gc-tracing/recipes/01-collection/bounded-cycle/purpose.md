# Reclaim an unreachable cycle

This language-neutral managed-graph policy specimen documents the bounded
collector call: the arena graph is fully planned at a safepoint, and only an
admitted plan reaches atomic sweep. The Rust conformance specimen generates
object graphs and legal safepoint schedules without installing or claiming a
language capability; it also runs collection inside a wasm-compatible closure.
