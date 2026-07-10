# Clojure codec profile (descriptor)

This documents the `clojure` codec profile: the reader/printer surface and the language
features it presents (EDN literals, namespaces, sequences, and recur). A codec profile is runtime metadata describing a surface,
not an expression that reduces to a value, so it is documented rather than evaluated. The
profile's behavior is exercised by the Clojure conformance suite (`cargo test`).
