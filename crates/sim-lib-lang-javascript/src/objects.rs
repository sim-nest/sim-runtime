//! JavaScript object policy over the shared property and managed-identity organs.

use crate::{
    JavascriptHeap, JavascriptHeapExt, JavascriptManagedKind, JavascriptManagedMutationError,
    JavascriptManagedObject, JavascriptValue,
};
use sim_kernel::Symbol;
use sim_lib_dispatch::{
    AccessContext, AccessError, AccessorDescriptor, DataDescriptor, DefineError, Descriptor,
    PropertyHook, PropertyStore,
};
use sim_lib_function::{
    CapturedBinding, FunctionPlan, InstanceError, ParameterKind, validate_capture_bindings,
};
use sim_lib_mutation::{ArenaError, ManagedHandle, ManagedId};
use std::collections::{BTreeMap, HashSet};

// Lexical partitions retain one module-private invariant surface while keeping
// each source unit reviewable under the repository size policy.
include!("objects/function.rs");
include!("objects/space.rs");
include!("objects/tests.rs");
