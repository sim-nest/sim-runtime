# sim-lib-lang-python

Embedded, capability-scoped, agent-authored Python scripting over SIM values.
It is deliberately not a CPython replacement. Thin, bounded Python core
evaluation enters through
`codec/python`; the profile directly interprets the stable lowering and composes
shared runtime organs, numbers, managed arena, and optional tracing collection.

`PYTHON_FIDELITY` reports syntax, lowering, direct evaluation, object/control,
module/library, boundedness, and expected gaps separately. `PYTHON_EVIDENCE_CASES`
publishes frozen call, cycle, exception, generator, matching, import, and
capability-refusal cases. CPython 3.14.6 supplied offline expected values for
simple differential cases only; it is never linked, imported, spawned, or used
as a fallback. There is no bytecode, compiler IR, optimizer, project graph, or
private language organ.

## Capability-gated `eval`, `exec`, and imports

Dynamic Python is safer than ambient CPython evaluation by construction.
`dynamic_python_policy` builds both `eval` and `exec` entries over the shared source policy
and the canonical diminished read-eval broker. The caller must supply a trusted
`ReadPolicy`, already hold every required capability, and choose the smaller
capability set visible to decoded code. Source text cannot mint authority.

`python_module_policy` likewise builds the shared policy that resolves only through a caller-supplied `Dir`,
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
