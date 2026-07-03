# sim-lib-lang-ruby

In one line: It lets you write for SIM in an expressive Ruby style, with the readable, block-friendly flavor Ruby is known for.

## What it gives you

This library gives SIM a Ruby-flavored face. Ruby is prized for reading almost like plain description and for its comfortable use of blocks, and this profile brings that expressive feel to SIM as a small language surface. You can shape your ideas in that fluent style and have them run on the same runtime as everything else. It is a front for reading and writing in the Ruby manner, not a separate interpreter hidden inside; the meaning is carried by SIM's shared expression graph beneath, so the pleasant notation sits on a common foundation.

## Why you will be glad

- The readable, block-friendly Ruby feel makes intentions easy to follow.
- People fond of Ruby get an expressive, familiar way to write for SIM.
- Your code runs on the shared runtime rather than a separate side engine.

## Where it fits

The kernel defines the codec, evaluation, and expression contracts. This crate is a loadable language profile that presents a Ruby surface syntax over the shared expression graph. It is one of several language faces the runtime can present, all reading into the same underlying forms, so a Ruby-styled program meets programs written in other styles on common ground.
