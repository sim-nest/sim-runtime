# Embedded, capability-scoped ECMAScript with explicit job drains

In one line: It runs bounded ECMAScript over SIM's shared values and organs without smuggling in a browser, Node, or ambient event loop.

## What it gives you

The loadable JavaScript profile evaluates lowered `codec/javascript` forms directly. Arrays, iterators, Map, Set, Symbol, descriptors, completions, modules, promises, and an exact UTF-16 string face compose the shared sequence, dispatch, control, namespace, mutation, and collection organs instead of forming a private VM. Promise and module work advances only at an explicit bounded drain-to-empty checkpoint. Hosts may deliberately supply capability-checked module roots and dynamic-source authority; source never acquires filesystem, process, network, timers, DOM, `fetch`, npm, CommonJS, or other host powers by implication.

RegExp execution is deliberately narrower than parsing: literals, dot, anchors, simple classes, ASCII shorthand classes, and greedy or lazy `?`, `*`, and `+` use the bounded pattern organ. Flags and advanced ECMAScript pattern constructs fail explicitly rather than being approximated.

## Why you will be glad

- Agent-authored JavaScript participates in SIM's common data, effects, capabilities, and replay evidence.
- Explicit job drains make scheduling visible and testable.
- Parser coverage, evaluator coverage, and known gaps are reported independently.
- UTF-16, JSON hooks, property order, cycles, and unsupported RegExp behavior have checked policy instead of host-runtime accidents.

## Where it fits

This crate is the loadable execution profile above `sim-codec-javascript` and the standard runtime organs. It is not a compiler, Realm engine, browser shim, Node adapter, or host event loop. Broader pattern coverage belongs in the shared pattern engine, not a hidden JavaScript-only implementation.
