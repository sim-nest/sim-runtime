# sim-lib-lang-islisp

In one line: It lets you write for SIM in ISLISP, the small standardized Lisp with a deliberately compact core.

## What it gives you

This library gives SIM an ISLISP face. ISLISP is a trim, standardized member of the Lisp family, built around a small and clearly defined core rather than a sprawling set of features. Writing in this profile means working with that compact, dependable vocabulary, which many people value for its clarity and its stable definition. It is a front for reading and writing in that style, not a separate interpreter tucked away; the meaning is carried by SIM's shared expression graph beneath. You get the tidy ISLISP notation while joining the same runtime as every other SIM surface.

## Why you will be glad

- The small, standardized core keeps the notation clear and predictable.
- People who prefer a compact Lisp get a comfortable way to work.
- What you write runs on the shared runtime rather than a side interpreter.

## Where it fits

The kernel defines the codec, evaluation, and expression contracts. This crate is a loadable language profile that presents ISLISP surface syntax over the shared expression graph. It is one of several language faces the runtime can present, all reading into the same underlying forms, so an ISLISP-styled program meets the others on common ground without conversion.
