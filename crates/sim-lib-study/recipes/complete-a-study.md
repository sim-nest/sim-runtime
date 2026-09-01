# Complete a study

1. Build `Selectors` from explicit content identities and call `expand`.
2. Seal the result with `SealPolicy`, a source assertion, bounds, and a parsed
   invocation `Datum`.
3. Create `StudyLifecycle` over a durable `JournalBackend` and call `install`.
4. Repeatedly call `run_one` with a `StudyExecutor` until the projection is
   complete.

The executor receives exactly one sealed coordinate and a cancellation view.
It returns typed, content-bound attempt evidence; it never mutates the journal.
