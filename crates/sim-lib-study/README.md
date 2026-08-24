# sim-lib-study

`sim-lib-study` is the domain-neutral coordinator for durable studies. It
expands selectors without effects, seals exact coordinates and invocation data,
claims work under a journal fence, executes through an injected `StudyExecutor`,
and reconstructs its projections solely from journal facts.

Its `design` module stages trial spend without turning exploration into proof:
the complete pool and policies are sealed first, smoke is diagnostic only,
screening and expected-decision-change selection are replayable exploratory
steps, and confirmation uses a separately sealed fixed matrix. Deterministic
common-task/sample blocks, conservative exposure admission, and immutable
epochs keep scheduling, budgets, and drift explicit.

The crate deliberately does not own model, network, process, or executor
effects. An executor owns and reports every effect it performs.
