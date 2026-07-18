# sim-runtime

In one line: the library set that gives a SIM process its everyday behavior, names, dispatch, control, logic, and language surfaces.

## What it gives you

sim-runtime turns the small SIM kernel into a usable running environment. It
provides the shared installation path for libraries, the organs that handle
binding, control flow, dispatch, patterns, sequences, mutation, namespaces, and
logic, plus the standard bundle that records capabilities, claims, fidelity, and
conformance evidence.

It also gathers the language profiles that let the same expression graph be
written from several familiar surfaces. The result is one runtime layer that can
be inspected, tested, and loaded consistently instead of many disconnected
helpers.

## Why you will be glad

- A fresh SIM process gets practical behavior without bloating the kernel.
- Multiple language surfaces share one checked runtime substrate.
- Claims and conformance evidence travel with the libraries that publish them.

## Where it fits

The kernel owns the contracts and data types; sim-runtime supplies the default
behaviors that implement those contracts. Command-line tools, host surfaces, and
domain libraries load these crates when they need the ordinary runtime organs
and language profiles a SIM program expects.
