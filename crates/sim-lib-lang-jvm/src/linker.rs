//! Exact, revision-bound identity and state for JVM bootstrap linkage sites.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Weak},
};

use sim_kernel::{
    Args, Callable, CapabilityName, ClassId, ClassRef, Cx, Error, Object, ObjectCompat, Ref,
    ShapeRef, Symbol, Value,
};
use sim_lib_class::{
    ClassDescriptor, ClassDescriptorInput, ClassIdentity, DeclaredParent, DescriptorClass,
    MemberShape, OpenMetadataEntry,
};
use sim_lib_function::{
    BoundCall, CapturedBinding, FunctionBodyPolicy, FunctionInstance, FunctionPlan,
};
use sim_lib_mutation::{ManagedHandle, RootedHandle};
use sim_shape::AnyShape;

use crate::{
    ClassDefinition, ClassDefinitionId, ClassLoader, ClassSpaceRevision, ConstantResolutionError,
    ConstantResolutionKind, InvocationError, JavaMember, JvmGraphError, JvmHeap, JvmValue,
    ResolutionCache,
};

// Lexical partitions retain one module-private invariant surface while keeping
// each source unit reviewable under the repository size policy.
include!("linker/adaptation.rs");
include!("linker/direct_handle.rs");
include!("linker/bootstrap_model.rs");
include!("linker/lambda_factory.rs");
include!("linker/functional_interface.rs");
include!("linker/linkage_cache.rs");
include!("linker/tests.rs");
