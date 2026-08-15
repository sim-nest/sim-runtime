# Characterize a migration

Declare a bounded scenario entirely in public semantic terms. Capture its
canonical observation before changing the implementation, make the refactor,
then capture the same scenario and compare with one named projection.

Matching content identities prove the contract is unchanged. A strict mismatch
retains its stable path, both canonical values, and any semantic failure
location. Do not capture debug rendering, host paths, roadmap identifiers, or
private implementation fields. The checked SDK characterization specimen
executes this before-and-after workflow through `sim::characterization`.
