# Destructure a list with match

`match` is the pattern organ: it evaluates the scrutinee once, then tries each
clause's pattern in order. The list pattern `[a b]` destructures the two-element
list `[1 2]`, binding `a` to `1` and `b` to `2`; the body returns `a`, so the
result is `1`.
