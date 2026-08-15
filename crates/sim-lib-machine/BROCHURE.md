# Neutral instruction-machine contracts

`sim-lib-machine` gives runtime authors a small, policy-parametric boundary for
bounded decoded-instruction machines. The crate freezes the seams for
instructions, logical value widths, effects, frames, handlers, managed roots,
safepoints, effect-free admission, and deterministic receipts while leaving
every guest-specific decision to consumers.
