# sim-lib-namespace

In one line: It organizes names into separate modules so large programs do not trip over each other.

## What it gives you

As a program grows, many parts want to use short, natural names, and those names start to collide. This library keeps them apart. It lets each area of code hold its own set of names, decide which ones to share, and pull in names from elsewhere on clear terms. You can bring in another module's names as they are, rename them to avoid a clash, or deliberately shadow one with your own. Because the rules for importing, exporting, and renaming are explicit, you always know where a given name came from and what it refers to.

## Why you will be glad

- Separate modules keep short names from colliding across a big codebase.
- Imports can be renamed or scoped, so borrowed names never force a conflict.
- Every name has a clear origin, which makes reading unfamiliar code easier.

## Where it fits

The kernel defines the registry and operation contracts that record what exists and how it is reached. This crate is the concrete namespace organ on top: modules, packages, import options, and the handling of export, rename, and shadow. Every language surface that offers modules leans on it, so the way names are grouped and shared stays uniform throughout the runtime.
