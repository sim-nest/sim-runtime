# Typed-lazy codec profile (descriptor)

This documents the `typed-lazy` codec profile: the reader/printer surface and the language
features it presents (option types, lazy references, and typeclasses). A codec profile is runtime metadata describing a surface,
not an expression that reduces to a value, so it is documented rather than evaluated. The
profile's behavior is exercised by the Typed-lazy conformance suite (`cargo test`).
