# sim-lib-lang-julia

In one line: It lets you write for SIM in Julia style, the notation favored for technical and numerical work.

## What it gives you

This library gives SIM a Julia face. Julia's notation is popular with people doing calculation-heavy and scientific work, and this profile lets you express your ideas in that familiar shape and run them on SIM. It is a front for reading and writing in the Julia style, not a separate engine copied inside; the meaning of what you write is carried by SIM's shared expression graph underneath. So you keep the clean, math-friendly look you already know while your code joins the same runtime that every other SIM surface shares, side by side with them.

## Why you will be glad

- People at home in Julia can write in a familiar, calculation-friendly notation.
- Your code runs on the shared runtime, not a separate side engine.
- The technical, numeric style carries over into real SIM programs.

## Where it fits

The kernel defines the codec, evaluation, and expression contracts. This crate is a loadable language profile that presents Julia surface syntax over the shared expression graph. It is one of several language faces the runtime can wear, all reading into the same underlying forms, so a Julia-styled program meets programs written in other styles on common ground.
