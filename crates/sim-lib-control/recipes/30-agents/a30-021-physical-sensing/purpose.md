# Physical-sensing trace (descriptor)

A deterministic, offline synthetic trace of a physical-sensing control loop: windowed sensor
averaging feeding a proportional controller with deadband and hysteresis. It demonstrates the
pipeline shape; the fake-sensor read and control-output effects run through the effect ledger
outside the sandbox eval stack, so the trace is documented rather than executed live.
