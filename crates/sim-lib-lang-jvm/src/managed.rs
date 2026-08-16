//! Managed JVM object-graph policy over the shared tracing substrate.

use sim_lib_gc_tracing::{CollectionError, CollectionLimits, CollectionReceipt, ManagedHeap};
use sim_lib_mutation::{
    ArenaError, EdgeAllocationError, EdgeId, EphemeronMutationError, ManagedHandle, ManagedNode,
    RootedHandle, StrongEdgeMutationError, WeakEdgeMutationError,
};

/// Every role carried by a managed JVM node.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JvmRole {
    /// An ordinary instance and its reference fields.
    Object,
    /// A reference array and its elements.
    Array,
    /// A loaded class mirror and its defining state.
    ClassMirror,
    /// A class loader and the classes it defines.
    Loader,
    /// Static reference storage owned by one class.
    Statics,
    /// A managed Java string.
    String,
    /// A managed primitive wrapper value.
    PrimitiveBox,
    /// A throwable and its causal diagnostic graph.
    Throwable,
    /// Monitor bookkeeping associated with an object.
    Monitor,
    /// A verified, prepared method.
    PreparedMethod,
    /// A derived resolution or intern cache.
    Cache,
}

/// JVM edge semantics. Collection kind is fixed by [`JVM_ROLE_EDGE_TABLE`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JvmEdge {
    /// Ordinary reference field.
    Field,
    /// Reference-array element.
    Element,
    /// Instance class mirror.
    Class,
    /// Class's defining loader.
    DefiningLoader,
    /// Class's static storage.
    StaticStorage,
    /// Class supertype or interface mirror.
    Supertype,
    /// Class-owned prepared method.
    Method,
    /// Loader-defined class.
    DefinedClass,
    /// Static reference value.
    StaticValue,
    /// String backing array.
    StringStorage,
    /// Throwable cause.
    Cause,
    /// Throwable suppressed exception.
    Suppressed,
    /// Throwable detail message.
    DetailMessage,
    /// Monitor's associated object, held weakly.
    MonitoredObject,
    /// Prepared method's declaring class.
    DeclaringClass,
    /// Prepared method constant or resolved dependency.
    MethodDependency,
    /// Cache key and derived value, represented as one ephemeron.
    DerivedEntry,
}

/// Complete edge policy for one JVM role.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JvmRoleEdges {
    /// Role whose outgoing graph is described.
    pub role: JvmRole,
    /// Retaining edge semantics admitted for this role.
    pub strong: &'static [JvmEdge],
    /// Non-retaining edge semantics admitted for this role.
    pub weak: &'static [JvmEdge],
    /// Key-dependent edge semantics admitted for this role.
    pub ephemeron: &'static [JvmEdge],
}

/// Exhaustive JVM managed-edge table, indexed in [`JvmRole`] declaration order.
pub const JVM_ROLE_EDGE_TABLE: &[JvmRoleEdges] = &[
    JvmRoleEdges {
        role: JvmRole::Object,
        strong: &[JvmEdge::Field, JvmEdge::Class],
        weak: &[],
        ephemeron: &[],
    },
    JvmRoleEdges {
        role: JvmRole::Array,
        strong: &[JvmEdge::Element, JvmEdge::Class],
        weak: &[],
        ephemeron: &[],
    },
    JvmRoleEdges {
        role: JvmRole::ClassMirror,
        strong: &[
            JvmEdge::DefiningLoader,
            JvmEdge::StaticStorage,
            JvmEdge::Supertype,
            JvmEdge::Method,
        ],
        weak: &[],
        ephemeron: &[],
    },
    JvmRoleEdges {
        role: JvmRole::Loader,
        strong: &[JvmEdge::DefinedClass],
        weak: &[],
        ephemeron: &[],
    },
    JvmRoleEdges {
        role: JvmRole::Statics,
        strong: &[JvmEdge::StaticValue],
        weak: &[],
        ephemeron: &[],
    },
    JvmRoleEdges {
        role: JvmRole::String,
        strong: &[JvmEdge::StringStorage, JvmEdge::Class],
        weak: &[],
        ephemeron: &[],
    },
    JvmRoleEdges {
        role: JvmRole::PrimitiveBox,
        strong: &[JvmEdge::Class],
        weak: &[],
        ephemeron: &[],
    },
    JvmRoleEdges {
        role: JvmRole::Throwable,
        strong: &[
            JvmEdge::Cause,
            JvmEdge::Suppressed,
            JvmEdge::DetailMessage,
            JvmEdge::Class,
        ],
        weak: &[],
        ephemeron: &[],
    },
    JvmRoleEdges {
        role: JvmRole::Monitor,
        strong: &[],
        weak: &[JvmEdge::MonitoredObject],
        ephemeron: &[],
    },
    JvmRoleEdges {
        role: JvmRole::PreparedMethod,
        strong: &[JvmEdge::DeclaringClass, JvmEdge::MethodDependency],
        weak: &[],
        ephemeron: &[],
    },
    JvmRoleEdges {
        role: JvmRole::Cache,
        strong: &[],
        weak: &[],
        ephemeron: &[JvmEdge::DerivedEntry],
    },
];

/// Failure to add an edge inconsistent with the exhaustive role table.
#[derive(Debug)]
pub enum JvmGraphError {
    /// The owner or target handle is stale.
    Arena(ArenaError),
    /// The edge semantic is not valid for the owner's role and collection kind.
    InvalidEdge {
        /// Role of the prospective edge owner.
        role: JvmRole,
        /// Rejected edge semantic.
        edge: JvmEdge,
    },
    /// Shared-node edge allocation failed.
    Allocation(EdgeAllocationError),
    /// A compare-and-mutate edge operation found stale graph state.
    StrongMutation(StrongEdgeMutationError),
}

impl From<ArenaError> for JvmGraphError {
    fn from(value: ArenaError) -> Self {
        Self::Arena(value)
    }
}
impl From<StrongEdgeMutationError> for JvmGraphError {
    fn from(value: StrongEdgeMutationError) -> Self {
        match value {
            StrongEdgeMutationError::Allocation(e) => Self::Allocation(e),
            other => Self::StrongMutation(other),
        }
    }
}
impl From<WeakEdgeMutationError> for JvmGraphError {
    fn from(value: WeakEdgeMutationError) -> Self {
        match value {
            WeakEdgeMutationError::Allocation(e) => Self::Allocation(e),
            _ => unreachable!("insertion only reports allocation failures"),
        }
    }
}
impl From<EphemeronMutationError> for JvmGraphError {
    fn from(value: EphemeronMutationError) -> Self {
        match value {
            EphemeronMutationError::Allocation(e) => Self::Allocation(e),
            _ => unreachable!("insertion only reports allocation failures"),
        }
    }
}

/// JVM heap composed directly from shared managed nodes and tracing collection.
pub struct JvmHeap {
    heap: ManagedHeap<ManagedNode<JvmRole>>,
}

impl JvmHeap {
    /// Creates a bounded tracing JVM heap.
    pub fn new(cap: usize, limits: CollectionLimits) -> Result<Self, ArenaError> {
        Ok(Self {
            heap: ManagedHeap::tracing(cap, limits)?,
        })
    }

    /// Allocates an empty shared node with the given JVM role.
    pub fn allocate(&mut self, role: JvmRole) -> Result<ManagedHandle, ArenaError> {
        self.heap.allocate(ManagedNode::new(role))
    }

    /// Roots a node.
    pub fn root(&mut self, handle: ManagedHandle) -> Result<RootedHandle, ArenaError> {
        self.heap.root(handle)
    }

    /// Releases a root.
    pub fn release_root(&mut self, root: RootedHandle) -> Result<ManagedHandle, ArenaError> {
        self.heap.release_root(root)
    }

    /// Returns the number of live nodes.
    pub fn live_len(&self) -> usize {
        self.heap.live_len()
    }

    /// Adds a retaining edge admitted by the owner's role.
    pub fn strong(
        &mut self,
        owner: ManagedHandle,
        edge: JvmEdge,
        target: ManagedHandle,
    ) -> Result<EdgeId, JvmGraphError> {
        self.heap.get(target)?;
        let node = self.heap.get_mut(owner)?;
        if !policy(*node.role()).strong.contains(&edge) {
            return Err(JvmGraphError::InvalidEdge {
                role: *node.role(),
                edge,
            });
        }
        Ok(node.insert_strong(target.id())?)
    }

    /// Replaces a retaining edge after validating both live handles.
    pub fn replace_strong(
        &mut self,
        owner: ManagedHandle,
        edge: EdgeId,
        expected: ManagedHandle,
        replacement: ManagedHandle,
    ) -> Result<(), JvmGraphError> {
        self.heap.get(replacement)?;
        self.heap
            .get_mut(owner)?
            .replace_strong(edge, expected.id(), replacement.id())?;
        Ok(())
    }

    /// Removes a retaining edge after validating the expected target.
    pub fn remove_strong(
        &mut self,
        owner: ManagedHandle,
        edge: EdgeId,
        expected: ManagedHandle,
    ) -> Result<(), JvmGraphError> {
        self.heap
            .get_mut(owner)?
            .remove_strong(edge, expected.id())?;
        Ok(())
    }

    /// Adds a weak edge admitted by the owner's role.
    pub fn weak(
        &mut self,
        owner: ManagedHandle,
        edge: JvmEdge,
        target: ManagedHandle,
    ) -> Result<EdgeId, JvmGraphError> {
        self.heap.get(target)?;
        let node = self.heap.get_mut(owner)?;
        if !policy(*node.role()).weak.contains(&edge) {
            return Err(JvmGraphError::InvalidEdge {
                role: *node.role(),
                edge,
            });
        }
        Ok(node.insert_weak(target.id())?)
    }

    /// Adds a key-dependent cache entry admitted by the owner's role.
    pub fn ephemeron(
        &mut self,
        owner: ManagedHandle,
        edge: JvmEdge,
        key: ManagedHandle,
        value: ManagedHandle,
    ) -> Result<EdgeId, JvmGraphError> {
        self.heap.get(key)?;
        self.heap.get(value)?;
        let node = self.heap.get_mut(owner)?;
        if !policy(*node.role()).ephemeron.contains(&edge) {
            return Err(JvmGraphError::InvalidEdge {
                role: *node.role(),
                edge,
            });
        }
        Ok(node.insert_ephemeron(key.id(), value.id())?)
    }

    /// Collects unreachable nodes and returns exact shared tracing evidence.
    pub fn collect(&mut self) -> Result<CollectionReceipt, CollectionError> {
        Ok(self
            .heap
            .collect()?
            .expect("JVM heaps always use tracing policy"))
    }
}

fn policy(role: JvmRole) -> &'static JvmRoleEdges {
    &JVM_ROLE_EDGE_TABLE[role as usize]
}
