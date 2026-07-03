# sim-lib-logic

In one line: It lets you state facts and rules, then ask questions and get every answer that fits.

## What it gives you

This library brings rule-based reasoning to SIM. You record facts and the rules that connect them, then pose a question with blanks in it, and the engine finds the values that make the question true. It works by matching shapes against each other and filling in the blanks consistently, and it can hold extra conditions that the answers must also satisfy. Results arrive as a stream you can take from one at a time, so a question with many answers stays manageable. It turns "what combinations satisfy all of this" into a direct thing you can ask.

## Why you will be glad

- You describe what is true and let the engine work out the answers.
- One question can yield many answers, delivered steadily as a stream.
- Extra conditions narrow the search, so results stay relevant.

## Where it fits

The kernel defines the Shape matching protocol along with eval and codec contracts. This crate is the concrete logic organ built on them: a clause database, a unifier, constraint solving, and a query surface. The Prolog language profile and other rule-driven features sit on top of this engine, so logical reasoning across the runtime shares one dependable core.
