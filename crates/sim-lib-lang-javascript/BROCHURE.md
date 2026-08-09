# Direct JavaScript, without an engine inside the engine

Run bounded JavaScript core programs as native SIM expressions through the
loadable JavaScript codec and profile, with explicit capabilities and shared
managed storage instead of Node or a foreign VM.

The standard core includes sparse arrays, iterators, Map, Set, Symbol, an exact
UTF-16 string face, and JavaScript JSON hooks over SIM's shared organs. RegExp is
strictly bounded in this release: literals, simple classes, anchors, dot, and
`? * +` execute; flags and advanced ECMAScript syntax fail explicitly. The
first successor is `JAVA_SCRIPT_6` pattern-engine work, not silent emulation.
