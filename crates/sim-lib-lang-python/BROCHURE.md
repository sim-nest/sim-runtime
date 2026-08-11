# Embedded, capability-scoped Python scripting

Run agent-authored Python over SIM values without a foreign runtime. This is a
bounded, tree-walking scripting profile, not a CPython replacement: there is no
`pip`, `asyncio` event loop, ambient IO, bytecode, compiler, or host import
search. Source enters through `codec/python`; dynamic `eval` and `exec` require
the caller's `read-eval` capability and an explicitly diminished authority set.

The public fidelity report separates syntax, lowering, direct evaluation,
object/control, module/library, boundedness, and expected gaps. Checked classes,
descriptors, exceptions, generators, matching, and cyclic values compose shared
runtime organs; imports resolve only through a caller-supplied directory.
