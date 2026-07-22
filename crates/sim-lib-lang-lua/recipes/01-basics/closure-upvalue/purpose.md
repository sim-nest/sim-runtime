# Lua closure upvalue case

This records the Lua source case where a nested function updates a surrounding
local and returns the accumulated result. The runtime conformance test evaluates
the source and checks the observed value.
