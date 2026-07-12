# Map a function over a list

`seq/map` is a sequence organ: it applies a function value to every element of a
list, driving the evaluator once per element. Here the function `(x) -> x * x`
squares each element of `[1 2 3]`, producing `(1 4 9)`.
