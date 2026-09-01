This recipe records a sealed host process run: a caller supplies a boot-trusted
program reference, whole arguments, an opaque project-root reference, exact
bindings, a timeout, an output limit, and optional stdin. The child environment
is empty unless a value is explicitly declared; neither callers nor receipts
expose a host path.

Retry table:

| attempt | automatic retry |
| --- | --- |
| `NotDispatched` | yes |
| `Completed` (including non-zero exit) | no |
| `StoppedAfterTimeout` | no |
| `StoppedAfterCancel` | no |
| `UnknownAfterDispatch` | no |
