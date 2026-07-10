# Common Lisp codec profile (descriptor)

This documents the `cl` codec profile: the reader/printer surface and the language
features it presents (packages, conditions, restarts, and generic functions). A codec profile is runtime metadata describing a surface,
not an expression that reduces to a value, so it is documented rather than evaluated. The
profile's behavior is exercised by the Common Lisp conformance suite (`cargo test`).
