# sim-lib-lang-cl

In one line: It lets you write for SIM in familiar Common Lisp style, with the parentheses and forms Lisp people expect.

## What it gives you

This library gives SIM a Common Lisp face. If you are comfortable with the Common Lisp way of writing -- its list forms, its naming habits, its long-standing conventions -- you can express your ideas that way and have them run on SIM. It is a front for reading and writing in that style, not a separate interpreter hiding inside; the actual meaning is carried by SIM's shared expression graph beneath. So you keep the notation you know while everything you write joins the same runtime that every other SIM surface shares.

## Why you will be glad

- People who know Common Lisp can be productive without learning a new notation.
- What you write runs on the shared runtime, not a separate side interpreter.
- The familiar list forms and conventions carry over to real SIM programs.

## Where it fits

The kernel defines the codec, evaluation, and expression contracts. This crate is a loadable language profile that presents Common Lisp surface syntax over the shared expression graph. It is one of several language faces SIM can wear, all reading down into the same underlying forms, so a program written in this style meets programs written in the others on common ground.
