# sim-lib-dispatch

In one line: It picks the right version of an operation based on the kinds of things you hand it.

## What it gives you

This library lets one named operation have many implementations and choose the fitting one automatically. You describe several versions of an operation, each meant for a particular kind of value, and when the operation is called this crate looks at the actual arguments and runs the version that matches best. When more than one version could apply, it settles ties in a clear, stated order rather than by chance. The result is that you can add new behavior for new kinds of data without editing the places that already call the operation, keeping code open to growth.

## Why you will be glad

- One operation name can serve many kinds of data without tangled branching.
- New cases plug in without touching existing callers or existing versions.
- Overlapping matches resolve in a defined order, so the choice is never a guess.

## Where it fits

The kernel defines what a callable is and how an operation is identified. This crate is the concrete organ that turns those contracts into generic functions and multimethods, complete with the ordering rules that decide which method wins. Language surfaces and libraries that want type-directed behavior route through it, so dispatch works the same way regardless of the syntax on top.
