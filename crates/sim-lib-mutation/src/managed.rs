use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

// Lexical partitions retain one module-private invariant surface while keeping
// each source unit reviewable under the repository size policy.
include!("managed/handles.rs");
include!("managed/edges.rs");
include!("managed/object.rs");
include!("managed/arena_model.rs");
include!("managed/arena.rs");
