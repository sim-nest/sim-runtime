# sim-lib-lang-scheme

In one line: It lets you write for SIM in Scheme, the small, clean Lisp dialect of the R7RS-small standard.

## What it gives you

This library gives SIM a Scheme face, following the R7RS-small standard. Scheme is admired for being minimal and clear, giving you a small set of well-chosen forms rather than a crowded feature list, and this profile brings that tidy style to SIM. You can write in the familiar Scheme shape and have it run on the shared runtime. It is a front for reading and writing in that style, not a separate interpreter tucked inside; the meaning is carried by SIM's shared expression graph underneath, so the clean notation rests on a common foundation.

## Why you will be glad

- The small R7RS-small core keeps the notation clear and easy to hold in mind.
- People who value a minimal Lisp get a comfortable, familiar surface.
- What you write runs on the shared runtime, not a bundled side interpreter.

## Where it fits

The kernel defines the codec, evaluation, and expression contracts. This crate is a loadable language profile that presents Scheme surface syntax over the shared expression graph. It is one of several language faces the runtime can wear, all reading into the same underlying forms, so a Scheme-styled program meets programs written in other styles on common ground.
