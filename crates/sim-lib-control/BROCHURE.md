# sim-lib-control

In one line: It manages how a running program moves -- pausing, resuming, retrying, and recovering when something goes wrong.

## What it gives you

This library shapes the flow of work inside SIM. It lets a computation pause and pick up later, hand values back and forth as it goes, explore several possibilities and back out of the ones that fail, and jump straight out of deep nesting when that is the clearest thing to do. It also handles trouble in an orderly way, offering named ways to recover instead of simply stopping. Together these turn awkward control situations -- long-running steps, search, cleanup, and error handling -- into ordinary, describable pieces you can reason about.

## Why you will be glad

- Long or paused work can resume where it left off without losing its place.
- Failed attempts can back out cleanly so a search can try the next option.
- Errors offer named recovery choices rather than crashing the whole run.

## Where it fits

SIM keeps the rules of control -- how execution may branch, suspend, or unwind -- in the kernel as contracts. This crate is the concrete organ that carries those rules out, providing coroutines, generators, restarts, and non-local exits for every surface above it. Language profiles and libraries reach for these behaviors instead of building their own, so flow control stays uniform across the whole runtime.
