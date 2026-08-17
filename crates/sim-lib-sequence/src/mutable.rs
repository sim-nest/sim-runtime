//! Neutral mutable storage for sparse indexed sequences and ordered keyed collections.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::ops::{Bound, RangeBounds};
use std::rc::Rc;

const CHUNK_LEN: usize = 64;

// Lexical partitions retain one module-private invariant surface while keeping
// each source unit reviewable under the repository size policy.
include!("mutable/ordered_table.rs");
include!("mutable/ordered_set.rs");
include!("mutable/sparse.rs");
include!("mutable/tests.rs");
