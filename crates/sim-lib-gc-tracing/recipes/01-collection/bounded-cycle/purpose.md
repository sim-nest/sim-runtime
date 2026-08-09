# Reclaim an unreachable cycle

This policy specimen documents the bounded collector call: the arena graph is
fully planned at a safepoint, and only an admitted plan reaches atomic sweep.
The Rust conformance specimen exercises the same limits against concrete cycles.
