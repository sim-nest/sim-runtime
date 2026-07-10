# ISLISP codec profile (descriptor)

This documents the `islisp` codec profile: the reader/printer surface and the language
features it presents (its reader and dispatch surface). A codec profile is runtime metadata describing a surface,
not an expression that reduces to a value, so it is documented rather than evaluated. The
profile's behavior is exercised by the ISLISP conformance suite (`cargo test`).
