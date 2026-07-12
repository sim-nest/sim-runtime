# Bind a local and square it

`let` is the binding organ: a special form that introduces a lexical scope. It
evaluates each initializer in the outer scope, installs the bindings in a fresh
child environment, and runs the body there. Here `x` is bound to `5` and the
body `(math/mul x x)` computes `25` -- the binding is gone once the form returns.
