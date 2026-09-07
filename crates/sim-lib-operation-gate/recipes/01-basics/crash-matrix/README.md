# Recover a dispatched operation

This recipe uses only the deterministic memory journal and an injected fake
performer. It persists intent and dispatch, loses the performer
acknowledgement, reopens the service under a fresh lease, and proves replay
does not repeat the already recorded dispatch. It performs no host process or
network effect.
