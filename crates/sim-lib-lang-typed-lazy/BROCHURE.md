# sim-lib-lang-typed-lazy

In one line: It lets you write for SIM in a typed, lazy style where values are checked ahead and computed only when needed.

## What it gives you

This library gives SIM a typed, lazily-evaluated face. Here the kinds of your values are checked before the program runs, so many mistakes are caught early as clear messages rather than surprises during a run. And work is put off until its result is actually wanted, which lets you describe long or open-ended computations and still pay only for the parts you use. This profile brings that careful, on-demand style to SIM as a small language surface. It is a front for reading and writing in that manner, with the meaning carried by SIM's shared expression graph beneath.

## Why you will be glad

- Type checks ahead of time turn a class of mistakes into early, clear messages.
- Computing only on demand lets you describe long work and pay for just a slice.
- The careful style rests on the shared runtime, beside every other surface.

## Where it fits

The kernel defines the codec, evaluation, and expression contracts. This crate is a loadable language profile that presents a typed, lazily-evaluated surface syntax over the shared expression graph. It is one of several language faces the runtime can present, all reading into the same underlying forms, so a typed, lazy program meets programs written in other styles on common ground.
