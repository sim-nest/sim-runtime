# Pattern dialect characterization baseline

This baseline reuses the landed `sim-lib-pattern` text VM and the
`sim-lib-standard-core` characterization capture contract. It introduces no
matcher, parser, cursor, diagnostic, or limit policy.

| Concern | Existing owner reused | Frozen evidence |
| --- | --- | --- |
| Lua syntax | `LuaPatternDialect` | accepted spans and captures; malformed-clause refusals |
| Glob syntax | `GlobPatternDialect` | anchored accepted and rejected matches; malformed-clause refusals |
| Matching | `run_text_pattern` | UTF-8 byte offsets, empty matches, greedy/lazy choice, and step exhaustion |
| JavaScript syntax | `JavascriptRegExp` | accepted spans; typed flag and syntax refusals |
| JavaScript gaps | `javascript_regexp_gaps` | the exact eight-entry public enum slice |
| Capture identity | `publish_characterization_capture` | identical inputs replay to identical content refs |

The captures project typed outcomes to canonical `Datum` records. Refusals use
stable clause names rather than host `Debug` text. Public diagnostic details
captured by the fixtures are checked to exclude roadmap-family names. The
JavaScript gap slice remains intact so later work can remove only clauses whose
behavior has actually landed.
