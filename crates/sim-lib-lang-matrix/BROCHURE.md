# sim-lib-lang-matrix

In one line: It gathers every language surface and checks them all against one shared standard so they agree.

## What it gives you

This library is the meeting point where SIM's language surfaces are held to a common bar. It assembles the surfaces together and runs them against a shared set of checks, so you can see at a glance whether each one reads and evaluates inputs the way it is meant to. When the surfaces all pass through the same examples, differences between them stand out plainly and agreement is confirmed rather than assumed. It gives the project one place to answer the question "do all our language fronts still behave consistently."

## Why you will be glad

- Every language surface is measured against the same shared checks.
- Disagreements between surfaces show up clearly instead of hiding.
- One assembly point answers whether the whole language family stays in step.

## Where it fits

This crate is the assembly point for the SIM language conformance matrix. It pulls in the language profiles and the generated inputs prepared elsewhere, then drives them through a shared battery of checks. It does not define the surfaces or invent the examples; it brings them together so the runtime can confirm, in one spot, that its many language faces remain consistent.
