# Shape binding surface

Live let/local binding needs the binding evaluator, which the cookbook's read-eval stack does not load. This recipe is a **descriptor** (tagged `sandbox-descriptor`): it shows the real
surface shape rather than a live result, because that result cannot be reproduced in
the cookbook's read-eval sandbox.
