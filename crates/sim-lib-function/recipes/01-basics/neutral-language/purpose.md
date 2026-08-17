# A function for a language that does not exist yet

The checked `neutral_language_specimen` test defines a tiny body policy in the
specimen itself. It gives that policy a lexical capture, one positional
parameter, one optional named parameter, explicit argument and result browsing
Shapes, and an opt-in generic-dispatch method. The function and its environment
form a managed cycle which the shared collector reclaims during the run.

Run it with:

```sh
cargo test -p sim-lib-function --test neutral_language_specimen
```

The companion ownership check reads the specimen source and rejects imports of
the established JavaScript, Python, Lua, and JVM guest runtimes. This is a
composition example for a future language, not another language implementation.
