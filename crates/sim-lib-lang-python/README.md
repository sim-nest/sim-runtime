# sim-lib-lang-python

Thin, bounded Python core evaluation for SIM. Source enters through
`codec/python`; the profile directly interprets the stable lowering and composes
shared runtime organs, numbers, managed arena, and optional tracing collection.

## Capability-gated `eval`, `exec`, and imports

Dynamic Python is safer than ambient CPython evaluation by construction.
`DynamicPython` sends both `eval` and `exec` through the installed source codec
and the canonical diminished read-eval broker. The caller must supply a trusted
`ReadPolicy`, already hold every required capability, and choose the smaller
capability set visible to decoded code. Source text cannot mint authority.

`PythonModulePolicy` likewise resolves only through a caller-supplied `Dir`,
then delegates decoding, failure/cycle caching, live exports, and receipts to
the shared namespace module lifecycle. It never searches host paths.

`python_library_manifest()` derives present members from the checked Python
matrix. Host-facing names such as `open`, `os`, `subprocess`, and `socket` are
explicit absences; they never fall back to CPython or ambient host services.
Structural cases compose ordered `Shape` checks and case-local captures. Guards
run only after a shape accepts, and unsupported Python pattern forms remain a
no-match gap instead of acquiring a second matcher.

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
