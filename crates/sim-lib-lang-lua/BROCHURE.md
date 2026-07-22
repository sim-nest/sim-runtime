# sim-lib-lang-lua

In one line: It lets SIM run Lua-shaped source on the shared runtime with clear boundaries around host effects.

## What it gives you

This library gives SIM a Lua face that reads source, evaluates ordinary chunks, and shares the same runtime organs as the other language profiles. Tables, closures, metatables, coroutines, string patterns, package lookup, math, and safe debug reporting all land as common SIM behavior instead of a separate embedded interpreter. Host-facing calls are guarded by capabilities, so scripts stay useful without gaining ambient authority.

## Why you will be glad

- Lua's small style gives newcomers a readable way into SIM programs.
- Scripts can use real source-level behavior while still sharing SIM's common data and effect model.
- Unsupported VM-only lanes are reported as named gaps, so users see the boundary instead of guessing.

## Where it fits

The kernel defines the codec, evaluation, and expression contracts. This crate is a loadable language profile that presents Lua surface syntax over the shared expression graph and standard runtime organs. It sits beside the other language faces, letting Lua-styled programs participate in the same conformance matrix, capability checks, and browseable profile evidence.
