# Capability-gated dynamic Python

Use `DynamicPython` for Python `eval` and `exec`. The host supplies the trusted
read policy, required caller powers, diminished `allow` set, and result Shape.
The source is decoded by the installed codec and admitted by the shared
read-eval broker; it receives no terminal, filesystem, process, or network
authority merely because it is Python.
