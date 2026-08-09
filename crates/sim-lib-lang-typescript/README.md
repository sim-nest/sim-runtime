# sim-lib-lang-typescript

TypeScript 7 notation over SIM's existing JavaScript evaluator. The TypeScript
codec performs direct erasure; this crate installs `language/typescript-notation`,
retains annotation provenance, and exposes only faithful browse-only Shape
metadata. It does not type-check, emit JavaScript, or provide a TypeScript runtime.

See `recipes/` for admitted execution, metadata browsing, and explicit gaps.
