// conformance: source module lifecycle and cross-organ language-neutral composition.

use std::{
    collections::BTreeMap,
    sync::{Arc, RwLock},
};

use sim_codec_lisp::LispCodecLib;
use sim_kernel::{
    ClassId, ClassRef, DefaultFactory, EagerPolicy, Object, ObjectCompat, Table, TrustLevel, Value,
    read_eval_capability,
};
use sim_lib_binding::{CallArgument, CallParameter, CallSignature};
use sim_lib_control::{
    AdmissionLimit, FrameLimits, JobQueues, ResumableFrame, ResumePacket, ResumeResult, WorkLimit,
};
use sim_lib_dispatch::{DataDescriptor, Descriptor, PropertyStore};
use sim_lib_mutation::{
    EdgeId, EdgeVisitor, HardCappedRetainPolicy, ManagedArena, ManagedId, ManagedObject,
};

use super::*;
use crate::{MAX_SPECIFIER_BYTES, MAX_SPECIFIER_CANDIDATES, SpecifierRefusalCode};
use sim_kernel::{CapabilitySet, ReadPolicy};

// Lexical partitions retain one module-private invariant surface while keeping
// each source unit reviewable under the repository size policy.
include!("tests/support.rs");
include!("tests/lifecycle.rs");
include!("tests/characterization.rs");
include!("tests/concurrency.rs");
