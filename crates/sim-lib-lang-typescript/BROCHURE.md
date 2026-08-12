# TypeScript notation, without a second runtime

In one line: It lets SIM run faithfully erasable TypeScript through the existing JavaScript evaluator while preserving type notation for inspection.

## What it gives you

The loadable profile installs `language/typescript-notation` over `codec/typescript`. Syntax whose erasure requires no checker or emitter becomes the same `javascript/*` graph used by ordinary JavaScript, so values, effects, modules, jobs, and capabilities retain one runtime meaning. Type annotations survive as browse-only, non-enforcing Shape metadata with source provenance. Constructs that need compiler judgment or produce JavaScript--such as checker-dependent assertions, enums, namespaces, parameter properties, decorators, and JSX transforms--remain located gaps and are never guessed into executable behavior.

## Why you will be glad

- TypeScript notation does not create a competing evaluator or effect model.
- Tools can browse annotations and derivation evidence without treating them as a hidden checker.
- Faithful erasure is explicit, while compiler-dependent syntax fails closed at its source location.
- JavaScript runtime improvements automatically benefit admitted TypeScript programs.

## Where it fits

This crate is the thin execution profile above `sim-codec-typescript` and `sim-lib-lang-javascript`. The codec owns TypeScript and TSX syntax fidelity; JavaScript owns runtime semantics; this profile owns the honest boundary between erasable notation, inspectable metadata, and named evaluation gaps.
