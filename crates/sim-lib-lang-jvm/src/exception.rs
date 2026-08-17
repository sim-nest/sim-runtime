//! Java throwable allocation, relations, and ordered exception-table dispatch.

use sim_codec_classfile::{InstructionId, Opcode};
use sim_kernel::{ClassRef, Cx, Result as KernelResult};
use sim_lib_control::{
    BoundedSubclassOutcome, ClassMatchBudget, ClassMatchOutcome, CleanupStack, ManagedException,
    Raised, RaisedUnwind, match_raised_class,
};
use sim_lib_gc_tracing::{CollectionError, CollectionLimits, CollectionReceipt, ManagedHeap};
use sim_lib_machine::{ManagedRootSource, StackError, UnitStack};
use sim_lib_mutation::{ArenaError, EdgeId, ManagedHandle, RootedHandle, StrongEdgeMutationError};

use crate::{JvmReference, JvmValue, JvmValueWidth, PreparedCatchEntry, PreparedJvmInstruction};

/// Java-owned relation roles stored on the shared managed exception adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JavaThrowableRelation {
    /// The initialization-once cause relation.
    Cause,
    /// One suppressed throwable, retained in insertion order.
    Suppressed,
}

/// Java policy retained as the payload of the shared managed exception adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JavaThrowableState {
    raised: Raised,
    cause_initialized: bool,
}

impl JavaThrowableState {
    /// Returns the one shared exceptional-completion envelope.
    pub const fn raised(&self) -> &Raised {
        &self.raised
    }

    /// Reports whether `initCause` has already made its one permitted decision.
    pub const fn cause_initialized(&self) -> bool {
        self.cause_initialized
    }
}

/// Java throwable heap composed from the shared adapter and tracing collector.
pub struct JavaThrowableHeap {
    heap: ManagedHeap<ManagedException<JavaThrowableState, JavaThrowableRelation>>,
}

impl JavaThrowableHeap {
    /// Creates a bounded tracing heap for Java throwable objects.
    pub fn new(capacity: usize, limits: CollectionLimits) -> Result<Self, ArenaError> {
        Ok(Self {
            heap: ManagedHeap::tracing(capacity, limits)?,
        })
    }

    /// Allocates a VM-raised Java object whose `Raised` class is its CLASS_2 face.
    pub fn allocate(&mut self, raised: Raised) -> Result<ManagedHandle, ArenaError> {
        self.heap
            .allocate(ManagedException::new(JavaThrowableState {
                raised,
                cause_initialized: false,
            }))
    }

    /// Returns the shared envelope carried by a live managed throwable.
    pub fn raised(&self, throwable: ManagedHandle) -> Result<&Raised, ArenaError> {
        Ok(self.heap.get(throwable)?.payload().raised())
    }

    /// Roots a throwable while it is held by a frame or host boundary.
    pub fn root(&mut self, throwable: ManagedHandle) -> Result<RootedHandle, ArenaError> {
        self.heap.root(throwable)
    }

    /// Releases one throwable root.
    pub fn release_root(&mut self, root: RootedHandle) -> Result<ManagedHandle, ArenaError> {
        self.heap.release_root(root)
    }

    /// Initializes the cause exactly once. Java permits self-cause here; it uses that value as
    /// the uninitialized sentinel internally, so exposing it as an edge would be incorrect.
    pub fn init_cause(
        &mut self,
        throwable: ManagedHandle,
        cause: ManagedHandle,
    ) -> Result<EdgeId, JavaThrowableMutationError> {
        self.heap.get(cause)?;
        let object = self.heap.get_mut(throwable)?;
        if object.payload().cause_initialized {
            return Err(JavaThrowableMutationError::CauseAlreadyInitialized);
        }
        if throwable == cause {
            return Err(JavaThrowableMutationError::SelfCause);
        }
        object.replace_payload(JavaThrowableState {
            raised: object.payload().raised.clone(),
            cause_initialized: true,
        });
        Ok(object.insert_relation(JavaThrowableRelation::Cause, cause.id())?)
    }

    /// Adds a suppressed throwable in Java insertion order, refusing self-suppression.
    pub fn add_suppressed(
        &mut self,
        throwable: ManagedHandle,
        suppressed: ManagedHandle,
    ) -> Result<EdgeId, JavaThrowableMutationError> {
        self.heap.get(suppressed)?;
        if throwable == suppressed {
            return Err(JavaThrowableMutationError::SelfSuppression);
        }
        Ok(self
            .heap
            .get_mut(throwable)?
            .insert_relation(JavaThrowableRelation::Suppressed, suppressed.id())?)
    }

    /// Returns relations in stable Java insertion order.
    pub fn relations(
        &self,
        throwable: ManagedHandle,
    ) -> Result<Vec<(JavaThrowableRelation, sim_lib_mutation::ManagedId)>, ArenaError> {
        Ok(self
            .heap
            .get(throwable)?
            .relations()
            .map(|(_, role, target)| (*role, target))
            .collect())
    }

    /// Collects unreachable throwable graphs, including unreachable cause cycles.
    pub fn collect(&mut self) -> Result<CollectionReceipt, CollectionError> {
        Ok(self
            .heap
            .collect()?
            .expect("Java throwable heaps always use tracing collection"))
    }

    /// Returns the current live throwable count.
    pub fn live_len(&self) -> usize {
        self.heap.live_len()
    }
}

/// Refusal from Java throwable relation policy.
#[derive(Debug)]
pub enum JavaThrowableMutationError {
    /// A managed handle was stale or outside this heap.
    Arena(ArenaError),
    /// The shared adapter could not allocate its retaining edge.
    Relation(StrongEdgeMutationError),
    /// Java cause initialization had already occurred.
    CauseAlreadyInitialized,
    /// Java refuses a throwable as its own cause.
    SelfCause,
    /// Java refuses `addSuppressed(this)`.
    SelfSuppression,
}

impl From<ArenaError> for JavaThrowableMutationError {
    fn from(value: ArenaError) -> Self {
        Self::Arena(value)
    }
}

impl From<StrongEdgeMutationError> for JavaThrowableMutationError {
    fn from(value: StrongEdgeMutationError) -> Self {
        Self::Relation(value)
    }
}

/// A selected JVM handler and the exact reference placed on its entry stack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JavaHandlerEntry {
    /// Classfile row selected by ordered search.
    pub row: usize,
    /// Prepared instruction at which the handler begins.
    pub instruction: InstructionId,
    /// The thrown managed object, the sole entry-stack value.
    pub throwable: JvmReference,
}

impl ManagedRootSource for JavaHandlerEntry {
    fn visit_managed_roots(
        &self,
        visit: &mut dyn FnMut(sim_lib_mutation::ManagedId) -> bool,
    ) -> bool {
        self.throwable.visit_managed_roots(visit)
    }
}

/// Typed failure during `athrow` or handler selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JavaThrowError {
    /// Execution was requested for something other than a prepared `athrow`.
    NotAthrow,
    /// The operand stack did not contain one reference.
    Stack(StackError),
    /// `athrow` was given a non-reference value.
    ExpectedReference,
    /// The thrown reference was null; the caller must allocate the VM NPE through this organ.
    NullReference,
    /// A catch type could not be resolved to its CLASS_2 class.
    UnresolvedCatchType(u16),
    /// Shared bounded matching refused malformed, exhausted, or failed policy evidence.
    Match(ClassMatchOutcome),
}

/// Mutable execution site consumed by one `athrow` operation.
pub struct JavaThrowSite<'a> {
    /// Prepared instruction at the throw point.
    pub instruction: &'a PreparedJvmInstruction,
    /// Current frame operand stack.
    pub operands: &'a mut UnitStack<JvmValueWidth>,
}

/// Executes `athrow` and performs ordered classfile handler search.
///
/// On a match the operand stack is replaced atomically with the exact JVMS handler-entry stack:
/// one reference to the thrown object. No JVM-local hierarchy walk occurs.
pub fn execute_athrow(
    cx: &mut Cx,
    site: JavaThrowSite<'_>,
    raised: &Raised,
    budget: ClassMatchBudget,
    mut catch_class: impl FnMut(u16) -> Option<ClassRef>,
    mut bounded_subclass: impl FnMut(
        &mut Cx,
        &ClassRef,
        &ClassRef,
        ClassMatchBudget,
    ) -> BoundedSubclassOutcome,
    mut jvm_predicate: impl FnMut(&mut Cx, &Raised, &ClassRef) -> KernelResult<bool>,
) -> Result<Option<JavaHandlerEntry>, JavaThrowError> {
    if site.instruction.opcode() != Opcode::Athrow {
        return Err(JavaThrowError::NotAthrow);
    }
    let value = site.operands.pop().map_err(JavaThrowError::Stack)?;
    let JvmValue::Reference(reference) = value else {
        return Err(JavaThrowError::ExpectedReference);
    };
    if reference.handle().is_none() {
        return Err(JavaThrowError::NullReference);
    }
    for handler in site.instruction.handler_membership() {
        if handler.catch_type == 0 {
            return enter_handler(site.operands, *handler, reference).map(Some);
        }
        let candidate = catch_class(handler.catch_type)
            .ok_or(JavaThrowError::UnresolvedCatchType(handler.catch_type))?;
        match match_raised_class(
            cx,
            raised,
            candidate,
            budget,
            &mut bounded_subclass,
            &mut jvm_predicate,
        ) {
            ClassMatchOutcome::Matched(_) => {
                return enter_handler(site.operands, *handler, reference).map(Some);
            }
            ClassMatchOutcome::NotMatched(_) => {}
            other => return Err(JavaThrowError::Match(other)),
        }
    }
    Ok(None)
}

fn enter_handler(
    operands: &mut UnitStack<JvmValueWidth>,
    handler: PreparedCatchEntry,
    throwable: JvmReference,
) -> Result<JavaHandlerEntry, JavaThrowError> {
    operands.clear();
    operands
        .push(JvmValue::Reference(throwable))
        .map_err(JavaThrowError::Stack)?;
    Ok(JavaHandlerEntry {
        row: handler.row,
        instruction: handler.handler,
        throwable,
    })
}

/// Runs every cleanup for an exceptional frame exit exactly once.
pub fn unwind_java_frame(
    raised: Raised,
    cleanups: CleanupStack<RaisedUnwind<(), (), ()>>,
) -> Raised {
    let RaisedUnwind::Exception(raised) = cleanups.unwind(RaisedUnwind::Exception(raised)) else {
        unreachable!("exception input preserves its unwind variant")
    };
    raised
}
