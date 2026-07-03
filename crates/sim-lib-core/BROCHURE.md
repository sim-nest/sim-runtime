# sim-lib-core

In one line: It is the shared plumbing every SIM library uses to announce what it offers and get it installed once, cleanly.

## What it gives you

This library is the quiet foundation the other libraries stand on. When a library wants to publish the values and behaviors it provides, it describes them as plain data and hands that description here. This crate installs those entries into the shared registry a single time, and doing it again changes nothing, so repeated setup never piles up duplicates or half-finished state. Because every library declares its offerings the same way, the whole collection stays consistent, easy to inspect, and easy to combine without each one reinventing the same bookkeeping.

## Why you will be glad

- Setup runs once and stays clean, even if it is triggered more than once.
- Every library declares what it offers in the same plain, inspectable form.
- Shared bookkeeping means individual libraries stay small and focused.

## Where it fits

The kernel defines the contracts for what a library is and how a registry records exported entries. This crate is the common substrate that turns those contracts into everyday use, giving the rest of the distribution one dependable way to declare exported cards and install them. Nearly every other organ in the runtime builds on it, which is why it stays deliberately small and steady.
