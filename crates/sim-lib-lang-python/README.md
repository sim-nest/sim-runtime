# sim-lib-lang-python

Thin, bounded Python core evaluation for SIM. Source enters through
`codec/python`; the profile directly interprets the stable lowering and composes
shared runtime organs, numbers, managed arena, and optional tracing collection.

`PythonObjectSpace` supplies checked class construction, C3 MRO, Python
data/non-data descriptor precedence, bound methods, and `super` while leaving
storage and interception in `sim-lib-dispatch`. `PythonGenerator` projects
start/send/throw/close over `sim-lib-control`'s bounded resumable frame; checked
exceptions, groups, chaining, and synchronous context cleanup add no scheduler.

Tracing collection is the default constructor. `PythonHeap::retaining` is an
explicit supported alternative and reports its strong-cycle leak gap.

The public `PYTHON_OBJECT_CONTROL_GAPS` list is the precise fail-closed boundary:
custom metaclasses, post-construction descriptor-protocol mutation,
weak-reference callbacks/proxies, `__del__` resurrection, and async scheduling
remain unclaimed.
