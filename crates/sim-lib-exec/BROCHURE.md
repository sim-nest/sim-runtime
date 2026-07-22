# sim-lib-exec

In one line: It lets a trusted host run a specific outside process with clear permission and tight limits.

## What it gives you

Some useful work belongs outside the runtime: a formatter, a compiler, a small command-line helper, or another tool the host already trusts. This crate gives that work a narrow gate. The caller names the exact program and arguments, the host checks permission first, and the run is bounded by a working directory root, a timeout, and a byte limit on captured output.

## Why you will be glad

- A process run is explicit about what starts and what authority allows it.
- Output, errors, and exit status come back in one predictable record.
- Time and output limits keep helper tools from taking over the session.

## Where it fits

The kernel carries the capability contract; this crate supplies the concrete host operation. Language libraries, table backends, build helpers, and supervised agents can use it when they need an outside process while keeping that process separate from SIM evaluation.
