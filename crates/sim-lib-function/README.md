# sim-lib-function

`sim-lib-function` owns the language-neutral plan and managed-instance mechanics
shared by guest function implementations. The frozen boundary and migration
ledger are in [CONTRACT.md](CONTRACT.md).

This initial crate is deliberately contract-only. Later FUNCTION_2 phases add
the implementation after the neutral/policy boundary has been reviewed and
checked.
