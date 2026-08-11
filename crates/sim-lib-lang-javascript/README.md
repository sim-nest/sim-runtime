# sim-lib-lang-javascript

Embedded, capability-scoped ECMAScript over lowered `codec/javascript` forms.
It evaluates directly and composes SIM's shared organs, installed Number/BigInt
domains, managed arena, and collector. Jobs run only at an explicit bounded
drain-to-empty checkpoint. It is not a compiler, VM, Node adapter, Realm engine,
DOM, timer service, ambient host-IO surface, or host event loop.

The public fidelity inventory scores syntax, lowering, direct evaluation,
objects, intrinsics, jobs/modules, boundedness, and expected gaps independently.
Its regression inventory names the checked descriptor, cycle, completion,
collection, job, module, RegExp, UTF-16, and capability-refusal specimens.

## Standard-core boundary

The checked manifest is generated from `intrinsics.tsv`; every admitted
constructor, namespace, prototype, and method names its canonical backing.
Arrays (including holes), iterators, Map, Set, and Symbol are JavaScript policy
over the sequence and mutation organs. Strings use a distinct UTF-16 code-unit
face, so indexing, slicing, and iteration preserve lone surrogates while
conversion to canonical scalar SIM text fails closed on an unpaired surrogate.
JSON parsing and encoding pass through `sim-codec-json`, with JavaScript-owned
reviver, `toJSON`, replacer, integer-key-first property order, undefined, and
cycle-rejection policy around that canonical owner.

RegExp literals parse, but v1 executes only this intersection with the bounded
pattern organ: literals, `.`, `^`/`$`, simple and negated character classes,
ASCII `\d`/`\s`/`\w` complements, and greedy or lazy `?`, `*`, and `+`.
Every flag (`d g i m s u v y`), alternation, groups, backreferences, lookaround,
Unicode property/set syntax, word-boundary assertions, and counted quantifiers
is rejected, never approximated. `JAVA_SCRIPT_6` pattern-engine work is the
first JavaScript successor requirement; until then this is deliberately not a
general ECMAScript RegExp implementation.
