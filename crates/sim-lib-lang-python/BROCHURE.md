# Embedded, capability-scoped Python scripting

In one line: It runs bounded, agent-authored Python over SIM values without importing CPython or granting ambient host authority.

## What it gives you

The profile directly interprets stable `codec/python` lowering and composes shared runtime organs for dispatch, control, namespaces, numbers, managed mutation, and optional tracing collection. Checked classes, C3 method resolution, descriptors, bound methods, exceptions, synchronous context cleanup, generators, structural matching, and cyclic values use those common contracts rather than a private Python VM.

Dynamic `eval` and `exec` pass through the canonical diminished read-eval broker. The caller must hold every required capability, provide trusted policy, and choose the smaller authority set visible to decoded code. Imports resolve only through a caller-supplied `Dir` and the shared namespace lifecycle; there is no host path search. `pip`, CPython bytecode, compiler IR, `asyncio`, ambient IO, and host modules such as `os`, `subprocess`, and `socket` are explicit absences.

## Why you will be glad

- Familiar Python notation can automate SIM while retaining SIM's capability and evidence model.
- Syntax, lowering, evaluation, objects, libraries, boundedness, and gaps are measured independently.
- Source cannot mint authority, and import behavior cannot wander into the host machine.
- Unsupported metaclass, weak-reference, finalizer-resurrection, and async lanes remain named fail-closed gaps.

## Where it fits

This is the loadable Python execution profile above `sim-codec-python`. The codec owns source fidelity; shared organs own storage and effects; this crate owns Python-facing policy and its public fidelity evidence. It is intentionally a safe scripting surface, not a CPython replacement.
