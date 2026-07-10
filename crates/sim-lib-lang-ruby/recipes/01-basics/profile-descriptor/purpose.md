# Ruby codec profile (descriptor)

This documents the `ruby` codec profile: the reader/printer surface and the language
features it presents (blocks, break/next, and dispatch). A codec profile is runtime metadata describing a surface,
not an expression that reduces to a value, so it is documented rather than evaluated. The
profile's behavior is exercised by the Ruby conformance suite (`cargo test`).
