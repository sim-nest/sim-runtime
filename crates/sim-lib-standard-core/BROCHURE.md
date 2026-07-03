# sim-lib-standard-core

In one line: It is the batteries-included default bundle that makes SIM useful the moment it starts.

## What it gives you

This library gathers the everyday behaviors most people expect a working system to already have. It sets up the permissions and claims that say who may do what, provides ways to compare two values and measure how faithfully one form was turned into another, and includes the harness that checks the system is behaving to standard. It brings the reading and construction of values from text, the Lisp surface for writing them, and the support that lets several language profiles share one core. In short, it wires the pieces together into a distribution you can pick up and use.

## Why you will be glad

- Common needs are present from the start, not assembled by hand each time.
- Comparing values and checking fidelity come ready, so results can be trusted.
- Reading, constructing, and multi-language support all share one settled core.

## Where it fits

The kernel defines the capability, claim, codec, and export contracts; this crate supplies the standard-distribution behavior that fills them in. It covers capabilities and claims, diff and fidelity, the conformance harness, install, language-profile support, the Lisp codec surface, polyglot support, and read and construct. It is the assembled default layer other tools and surfaces expect to find beneath them across the runtime.
