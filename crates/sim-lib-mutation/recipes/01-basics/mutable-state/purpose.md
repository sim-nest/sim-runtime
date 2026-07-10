# Mutable state (descriptor)

This documents the mutation organ's mutable containers (`cell`, `box`, `table`, `vector`,
`set`). Stateful mutation is recorded through the runtime's effect ledger, not the sandbox
eval stack, which evaluates pure forms -- so the organ is documented rather than run.
