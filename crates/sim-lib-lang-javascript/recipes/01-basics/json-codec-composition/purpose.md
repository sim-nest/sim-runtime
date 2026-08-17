# JavaScript policy over the canonical JSON codec

This specimen routes JSON text and tree mechanics through `sim-codec-json`.
The JavaScript wrapper retains only reviver, `toJSON`, replacer, property-order,
`undefined`, and cycle policy; it owns no parser or renderer.
