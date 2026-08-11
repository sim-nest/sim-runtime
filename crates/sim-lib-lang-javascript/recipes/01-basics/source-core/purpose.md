# Embedded ECMAScript with no ambient host loop

This recipe enters solely through `codec/javascript`, selects
`lang/javascript-core/v1`, and directly evaluates the stable lowering. Promise
and module jobs advance only at an explicit bounded drain-to-empty checkpoint.
It does not compile, emit instructions, invoke Node, or provide CommonJS, npm,
DOM, `fetch`, timers, filesystem, processes, network, ambient host IO, or an
implicit event loop.
