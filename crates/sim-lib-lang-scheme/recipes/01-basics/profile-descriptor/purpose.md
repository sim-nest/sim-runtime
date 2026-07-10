# Scheme codec profile (descriptor)

This documents the `scheme` codec profile: the reader/printer surface and the language
features it presents (the R7RS-small reader surface). A codec profile is runtime metadata describing a surface,
not an expression that reduces to a value, so it is documented rather than evaluated. The
profile's behavior is exercised by the Scheme conformance suite (`cargo test`).
