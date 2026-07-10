# Julia codec profile (descriptor)

This documents the `julia` codec profile: the reader/printer surface and the language
features it presents (Algol-style syntax and multiple dispatch). A codec profile is runtime metadata describing a surface,
not an expression that reduces to a value, so it is documented rather than evaluated. The
profile's behavior is exercised by the Julia conformance suite (`cargo test`).
