# sim-lib-study

`sim-lib-study` is the domain-neutral coordinator for durable studies. It
expands selectors without effects, seals exact coordinates and invocation data,
claims work under a journal fence, executes through an injected `StudyExecutor`,
and reconstructs its projections solely from journal facts.

The crate deliberately does not own model, network, process, or executor
effects. An executor owns and reports every effect it performs.
