# Completed Dataflow Analysis

This Lisp descriptor is the browse contract for a completed dataflow analysis.
The runtime projection exposes the graph, boundary states, work receipt, causal
explanations, continuation state, and content-bound proof identity through the
standard Table/Dir browse surface. The projection is minted only by consuming a
core completion proof; it has no public constructor or state mutation API.
