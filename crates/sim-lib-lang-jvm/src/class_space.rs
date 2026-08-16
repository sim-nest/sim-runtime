//! Authorized, lazy JVM class definition space.

use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};

use sim_codec_classfile::{ClassShell, Constant, ConstantSlot, ShellBudget};
use sim_kernel::{CapabilityName, CodecId, Cx, Dir, Error, Expr, Result, SourceId, Symbol};
use sim_lib_core::SourceAuthority;

static NEXT_LOADER_ID: AtomicU64 = AtomicU64::new(1);

// Lexical partitions retain one module-private invariant surface while keeping
// each source unit reviewable under the repository size policy.
include!("class_space/loader.rs");
include!("class_space/revision.rs");
include!("class_space/tests.rs");
