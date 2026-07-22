# Lua coroutine producer case

This records the Lua source case where a coroutine is created, resumed, and its
produced value is selected from the resume result. The runtime conformance test
keeps the coroutine behavior tied to the shared control organ.
