# sim-lib-sequence

In one line: It works with collections of items -- even endless ones -- without copying them over and over.

## What it gives you

This library is SIM's toolkit for series of values. It offers sequences that produce their items only as far as you actually read, so you can describe something long or even unending and still take just the first few. It offers collections that share their structure when you make a changed copy, so building a new version stays cheap instead of duplicating everything. And it offers pipelines that describe a chain of steps -- filter, map, combine -- once and apply them efficiently across a run. Together they let you shape and move data smoothly with little waste.

## Why you will be glad

- Long or endless series compute only as far as you actually look.
- Changed copies of a collection share what did not change, so edits stay cheap.
- Processing steps compose into one pass instead of many wasteful rebuilds.

## Where it fits

The kernel defines the operation and object contracts that say how values behave and are acted upon. This crate is the concrete sequence organ on top: lazy sequences, persistent vectors, maps, and sets, and transducer pipelines. Language surfaces and libraries that handle collections build on it, so working with series of data stays consistent and efficient across the whole runtime.
