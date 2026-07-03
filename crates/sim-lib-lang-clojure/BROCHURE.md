# sim-lib-lang-clojure

In one line: It lets you write for SIM in Clojure style, using EDN data notation and an immutable, functional feel.

## What it gives you

This library gives SIM a Clojure face. You can use the EDN data notation and the functional, data-first habits that Clojure programmers favor -- leaning on values that do not change and on plain data as the main shape of a program. It is a front for reading and writing in that style rather than a separate engine of its own; the meaning of what you write is carried by SIM's shared expression graph underneath. You keep the notation and the mindset you like while your code runs on the same runtime as every other SIM surface.

## Why you will be glad

- The EDN notation and data-first style carry straight over to SIM programs.
- People fond of immutable, functional code find a familiar way in.
- Your code runs on the shared runtime, meeting other surfaces on common ground.

## Where it fits

The kernel defines the codec, evaluation, and expression contracts. This crate is a loadable language profile that presents Clojure and EDN surface syntax over the shared expression graph. It is one of several language faces the runtime offers, all reading into the same underlying forms, so a Clojure-styled program sits beside programs written in other styles without translation trouble.
