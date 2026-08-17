# One callable protocol, three distinct object models

This checked specimen browses and calls equivalent Python, JavaScript, and Lua
functions through the kernel `Callable` protocol and compares their canonical,
content-identified outcome. The convergence deliberately stops there: Python
answers the declared-parent class query, JavaScript retains prototype lookup,
and Lua retains metatable `__index` lookup. The latter two are delegation edges,
not class lineage.

The executable assertion lives in the language-matrix crate's
`cross_language_specimen` test so all three guest profiles are observed in one
runnable artifact.
