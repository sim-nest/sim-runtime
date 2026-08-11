# Embedded, capability-scoped ECMAScript with explicit job drains

Run bounded ECMAScript as native SIM expressions through the loadable JavaScript
codec and profile. Promise and module jobs run only when the caller explicitly
drains them to empty. There is no implicit host loop and no Node, CommonJS, npm,
DOM, `fetch`, timers, filesystem, process, network, or ambient host IO.

That containment is the product: hosts may deliberately supply capability-
checked module roots and dynamic-source authority, while the language profile
uses shared SIM storage and control instead of hiding a foreign VM.

The standard core includes sparse arrays, iterators, Map, Set, Symbol, an exact
UTF-16 string face, and JavaScript JSON hooks over SIM's shared organs. RegExp is
strictly bounded in this release: literals, simple classes, anchors, dot, and
`? * +` execute; flags and advanced ECMAScript syntax fail explicitly. The
first successor is `JAVA_SCRIPT_6` pattern-engine work, not silent emulation.
