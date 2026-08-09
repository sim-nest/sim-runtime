# sim-lib-lang-python

Thin, bounded Python core evaluation for SIM. Source enters through
`codec/python`; the profile directly interprets the stable lowering and composes
shared runtime organs, numbers, managed arena, and optional tracing collection.

Tracing collection is the default constructor. `PythonHeap::retaining` is an
explicit supported alternative and reports its strong-cycle leak gap.
