# A bounded JVM runtime profile

In one line: It executes explicitly supplied JVM classfiles through checked verification, linking, managed objects, and capability-scoped SIM runtime services.

## What it gives you

The JVM profile consumes lossless classfile data and prepares it for bounded execution. It owns loader-local class spaces, verifier evidence, fields and arrays, initialization, invocation, exceptions, monitors, numeric instructions, bootstrap linkage, method handles, lambdas, string concatenation, frame pools, and guarded outcome caches. Every entry path carries resource limits and distinguishes admission, verification, linking, and execution failures. Class sources are supplied deliberately; there is no ambient classpath. Java strings retain exact code units, objects live in the shared managed heap, and guest failures use the common raised-value boundary. Browsing and fidelity reports expose what was proved and what the installed profile admits.

## Why you will be glad

- Classfile bytes do not execute until explicit verification and capability checks succeed.
- Loader, linkage, initialization, and cache identities remain visible and testable.
- Lambda and method-reference support reuses the neutral function organ instead of host closures.
- Bounded frames, arrays, monitors, and work receipts keep resource use accountable.

## Where it fits

This is the loadable Java Virtual Machine execution profile above `sim-codec-classfile`, the neutral machine, class, function, mutation, control, and text organs. It is not a host JVM wrapper and does not inherit filesystem, network, process, or class-loading authority from the machine running SIM.
