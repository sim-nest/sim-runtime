# sim-lib-lang-lua

In one line: It lets you write for SIM in Lua style, the small, approachable scripting notation many people already know.

## What it gives you

This library gives SIM a Lua face. Lua is known for being small, easy to pick up, and pleasant for scripting, and this profile lets you write in that light, readable style and have it run on SIM. It is a front for reading and writing in the Lua manner, not a separate interpreter carried along inside; the meaning is held by SIM's shared expression graph underneath. You get the gentle learning curve and clean look Lua is loved for, while what you write joins the same runtime that every other SIM surface uses.

## Why you will be glad

- Lua's small, approachable style makes an easy on-ramp for newcomers.
- Familiar scripting habits carry straight over into SIM programs.
- Your code runs on the shared runtime, not a bundled side interpreter.

## Where it fits

The kernel defines the codec, evaluation, and expression contracts. This crate is a loadable language profile that presents Lua surface syntax over the shared expression graph. It is one of several language faces the runtime can offer, all reading into the same underlying forms, so a Lua-styled program stands beside programs written in other styles on common ground.
