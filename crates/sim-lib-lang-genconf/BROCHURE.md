# sim-lib-lang-genconf

In one line: It generates a steady, repeatable set of test inputs used to confirm the language surfaces behave correctly.

## What it gives you

This library builds the raw material for checking that SIM's language surfaces do what they should. It walks the space of possible expressions in a fixed, orderly way and produces inputs that the conformance checks then feed through each surface. Because the walk is deterministic, the same inputs come out every time, so a result today can be compared against a result later with confidence that only the system changed, not the test set. It concentrates the tricky job of producing thorough, reproducible examples in one place, keeping the checks themselves simple.

## Why you will be glad

- The generated inputs are the same every run, so comparisons stay meaningful.
- Coverage is produced by rule, reaching cases a person might overlook.
- The hard part of making examples lives in one crate, not scattered about.

## Where it fits

This crate owns deterministic enumeration over the shared expression graph for the sake of conformance. The matrix runner and the per-language rows keep living in their own crates; this one simply supplies the generated inputs they consume. It is the quiet source of examples behind the checks that keep every language surface honest across the runtime.
