# Add on the Scheme surface

The Scheme (R7RS-small) reader parses `(+ 1 2)` in eval position and lowers it
to the call `(+ 1 2)`, which the runtime evaluates to `3`. Scheme is a
decode-only surface here, so the computed value is rendered in the canonical
lisp form -- the setup still parsed and ran on the Scheme surface.
