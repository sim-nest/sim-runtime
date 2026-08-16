//! JVM verification types and lawful dataflow frames.

use sim_incremental_core::dataflow::{JoinSemilattice, StateSize};
use std::{cell::RefCell, mem::size_of, sync::Arc};

use crate::{
    ClassDefinition, ClassDefinitionId, ClassLoader, ClassLoaderId, ClassSpaceRevision,
    JavaClassMetadata, JavaMember, JavaMemberKind,
};

/// One loaded class whose metadata was consulted by verification.
#[derive(Clone, Debug)]
pub struct VerificationDependency {
    class: Arc<ClassDefinition>,
    revision: ClassSpaceRevision,
}

impl VerificationDependency {
    /// Exact loaded definition observed by the query.
    pub fn class(&self) -> &ClassDefinitionId {
        self.class.id()
    }

    /// Class-space state in which the definition was observed.
    pub const fn revision(&self) -> ClassSpaceRevision {
        self.revision
    }
}

/// Failure to answer a bounded, read-only class-space query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationQueryError {
    /// The named class was not already loaded; verification never loads it.
    NotLoaded(String),
    /// The class-space changed while an observation was being recorded.
    ConcurrentRevision {
        /// Revision before the metadata lookup.
        before: ClassSpaceRevision,
        /// Revision after the metadata lookup.
        after: ClassSpaceRevision,
    },
    /// The caller's lineage-node allowance was exhausted.
    LineageLimit {
        /// Caller-supplied maximum consulted classes.
        limit: usize,
    },
    /// The environment's preallocated proof-dependency allowance was exhausted.
    DependencyLimit {
        /// Capacity fixed when the environment was created.
        limit: usize,
    },
}

/// Immutable verification-facing projection of one loaded class.
#[derive(Clone, Debug)]
pub struct VerificationClass {
    definition: Arc<ClassDefinition>,
}

impl VerificationClass {
    /// Content- and loader-bound class identity.
    pub fn id(&self) -> &ClassDefinitionId {
        self.definition.id()
    }

    /// Defining loader namespace.
    pub fn loader(&self) -> ClassLoaderId {
        self.definition.id().loader()
    }

    /// Neutral and JVM-specific class metadata.
    pub fn metadata(&self) -> &JavaClassMetadata {
        self.definition.metadata()
    }

    /// Declared interfaces, in classfile order.
    pub fn interfaces(&self) -> impl Iterator<Item = &str> {
        let skip_superclass = usize::from(!self.is_interface());
        self.metadata()
            .resolution()
            .direct_parents()
            .iter()
            .skip(skip_superclass)
            .map(String::as_str)
    }

    /// Whether this class carries `ACC_INTERFACE`.
    pub fn is_interface(&self) -> bool {
        self.metadata().access_flags() & 0x0200 != 0
    }

    /// Declared methods and constructors, in classfile order.
    pub fn methods(&self) -> impl Iterator<Item = &JavaMember> {
        self.metadata()
            .members()
            .iter()
            .filter(|member| member.kind() == JavaMemberKind::Method)
    }

    /// Declared fields, in classfile order.
    pub fn fields(&self) -> impl Iterator<Item = &JavaMember> {
        self.metadata()
            .members()
            .iter()
            .filter(|member| member.kind() == JavaMemberKind::Field)
    }
}

/// Result of a bounded verification assignability query.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationAssignability {
    /// The loaded declared lineage reaches the expected class identity.
    Assignable,
    /// The complete loaded declared lineage does not reach the expected class.
    NotAssignable,
}

/// Read-only, non-resolving view of a JVM class-loader namespace.
///
/// Dependency capacity is allocated once by [`Self::new`]. Queries only inspect
/// already-loaded definitions, append within that capacity, and never enter
/// class initialization, execution, native dispatch, source access, or ordinary
/// symbolic resolution.
pub struct VerificationEnvironment<'a> {
    loader: &'a ClassLoader,
    dependencies: RefCell<Vec<VerificationDependency>>,
    dependency_limit: usize,
}

impl<'a> VerificationEnvironment<'a> {
    /// Creates a view with a fixed proof-dependency allowance.
    pub fn new(loader: &'a ClassLoader, dependency_limit: usize) -> Self {
        Self {
            loader,
            dependencies: RefCell::new(Vec::with_capacity(dependency_limit)),
            dependency_limit,
        }
    }

    /// Defining loader namespace observed by this environment.
    pub fn loader(&self) -> ClassLoaderId {
        self.loader.id()
    }

    /// Exact, deduplicated dependencies accumulated by successful queries.
    pub fn dependencies(&self) -> impl std::ops::Deref<Target = [VerificationDependency]> + '_ {
        std::cell::Ref::map(self.dependencies.borrow(), Vec::as_slice)
    }

    /// Observes one already-loaded class without resolving or initializing it.
    pub fn class(&self, binary_name: &str) -> Result<VerificationClass, VerificationQueryError> {
        self.observe(binary_name)
    }

    /// Checks assignability through already-loaded declared superclass and
    /// interface metadata, charging at most `node_limit` consulted classes.
    pub fn is_assignable(
        &self,
        actual: &str,
        expected: &str,
        node_limit: usize,
    ) -> Result<VerificationAssignability, VerificationQueryError> {
        let mut remaining = node_limit;
        if self.lineage_reaches(actual, expected, node_limit, &mut remaining)? {
            Ok(VerificationAssignability::Assignable)
        } else {
            Ok(VerificationAssignability::NotAssignable)
        }
    }

    fn lineage_reaches(
        &self,
        binary_name: &str,
        expected: &str,
        limit: usize,
        remaining: &mut usize,
    ) -> Result<bool, VerificationQueryError> {
        if *remaining == 0 {
            return Err(VerificationQueryError::LineageLimit { limit });
        }
        *remaining -= 1;
        let class = self.observe(binary_name)?;
        if class.id().binary_name() == expected {
            return Ok(true);
        }
        for parent in class.metadata().resolution().direct_parents() {
            if self.lineage_reaches(parent, expected, limit, remaining)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn observe(&self, binary_name: &str) -> Result<VerificationClass, VerificationQueryError> {
        let before = self.loader.revision();
        let definition = self
            .loader
            .loaded(binary_name)
            .map_err(|_| VerificationQueryError::NotLoaded(binary_name.to_owned()))?
            .ok_or_else(|| VerificationQueryError::NotLoaded(binary_name.to_owned()))?;
        let after = self.loader.revision();
        if before != after {
            return Err(VerificationQueryError::ConcurrentRevision { before, after });
        }
        let mut dependencies = self.dependencies.borrow_mut();
        if !dependencies
            .iter()
            .any(|dependency| dependency.class.id() == definition.id())
        {
            if dependencies.len() == self.dependency_limit {
                return Err(VerificationQueryError::DependencyLimit {
                    limit: self.dependency_limit,
                });
            }
            dependencies.push(VerificationDependency {
                class: definition.clone(),
                revision: after,
            });
        }
        drop(dependencies);
        Ok(VerificationClass { definition })
    }
}

/// The width a verification value occupies in a local or operand frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationTypeWidth {
    /// One JVM slot.
    Category1,
    /// Two consecutive JVM slots.
    Category2,
}

/// Reference identity retained by bytecode verification.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ReferenceType {
    /// The universal reference supertype.
    Object,
    /// A loaded class or interface, named in internal JVM form.
    Class(Box<str>),
    /// An array whose component is itself a verification reference or primitive descriptor.
    Array(Box<str>),
}

/// A JVM verification type, ordered from [`Self::Bottom`] to [`Self::Unusable`].
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum VerificationType {
    /// No fact has reached this program point.
    Bottom,
    /// The category-1 integer family (`boolean`, `byte`, `char`, `short`, and `int`).
    Int,
    /// A category-1 IEEE-754 binary32 value.
    Float,
    /// A category-2 signed long value.
    Long,
    /// A category-2 IEEE-754 binary64 value.
    Double,
    /// The null reference, below every initialized reference type.
    Null,
    /// An initialized reference.
    Reference(ReferenceType),
    /// The distinguished receiver before its superclass constructor returns.
    UninitializedThis,
    /// An allocated reference identified by the bytecode offset of its `new` instruction.
    Uninitialized(u32),
    /// Conflicting or unusable information; the greatest lattice element.
    Unusable,
}

impl VerificationType {
    /// Returns the JVM slot width, or `None` when the value cannot occupy a frame.
    #[must_use]
    pub const fn width(&self) -> Option<VerificationTypeWidth> {
        match self {
            Self::Long | Self::Double => Some(VerificationTypeWidth::Category2),
            Self::Int
            | Self::Float
            | Self::Null
            | Self::Reference(_)
            | Self::UninitializedThis
            | Self::Uninitialized(_) => Some(VerificationTypeWidth::Category1),
            Self::Bottom | Self::Unusable => None,
        }
    }

    fn join_reference(left: &ReferenceType, right: &ReferenceType) -> ReferenceType {
        if left == right {
            left.clone()
        } else {
            ReferenceType::Object
        }
    }
}

impl StateSize for VerificationType {
    fn state_size(&self) -> usize {
        size_of::<Self>()
    }
}

impl JoinSemilattice for VerificationType {
    fn bottom(&self) -> Self {
        Self::Bottom
    }

    fn join(&self, other: &Self) -> Self {
        use VerificationType::{Bottom, Null, Reference, Unusable};
        match (self, other) {
            (Bottom, value) | (value, Bottom) => value.clone(),
            (Unusable, _) | (_, Unusable) => Unusable,
            (left, right) if left == right => left.clone(),
            (Null, Reference(reference)) | (Reference(reference), Null) => {
                Reference(reference.clone())
            }
            (Reference(left), Reference(right)) => Reference(Self::join_reference(left, right)),
            _ => Unusable,
        }
    }

    fn less_equal(&self, other: &Self) -> bool {
        self.join(other) == *other
    }
}

/// Internal slot representation exposed only so the frame's derived equality remains inspectable.
#[doc(hidden)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationSlot {
    /// A slot carrying no usable value.
    Unusable,
    /// The first slot of a verification value.
    Value(VerificationType),
    /// The second slot reserved by a category-2 value.
    Category2Tail,
}

use VerificationSlot as Slot;

/// Whether a frame describes locals or an operand stack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameKind {
    /// Random-access method locals.
    Locals,
    /// The ordered operand stack.
    OperandStack,
}

/// A refused frame mutation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    /// A value was written beyond the fixed frame capacity.
    OutOfBounds,
    /// A category-2 value did not have room for both slots.
    TruncatedCategory2,
    /// Operand-stack operations were requested from a locals frame or vice versa.
    WrongKind,
}

/// A locals or operand frame suitable for generic fixpoint dataflow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationFrame {
    /// No control-flow predecessor has reached this frame.
    Bottom {
        /// Frame role.
        kind: FrameKind,
        /// Fixed slot capacity.
        capacity: usize,
    },
    /// A reachable frame with an explicit slot layout.
    Reachable {
        /// Frame role.
        kind: FrameKind,
        /// Physical JVM slots.
        slots: Box<[VerificationSlot]>,
    },
}

impl VerificationFrame {
    /// Creates an unreachable frame with fixed shape.
    #[must_use]
    pub const fn bottom_frame(kind: FrameKind, capacity: usize) -> Self {
        Self::Bottom { kind, capacity }
    }

    /// Creates a reachable frame whose slots are initially unusable.
    #[must_use]
    pub fn new(kind: FrameKind, capacity: usize) -> Self {
        Self::Reachable {
            kind,
            slots: vec![Slot::Unusable; capacity].into_boxed_slice(),
        }
    }

    /// Returns the frame kind.
    #[must_use]
    pub const fn kind(&self) -> FrameKind {
        match self {
            Self::Bottom { kind, .. } | Self::Reachable { kind, .. } => *kind,
        }
    }

    /// Returns the fixed slot capacity.
    #[must_use]
    pub fn capacity(&self) -> usize {
        match self {
            Self::Bottom { capacity, .. } => *capacity,
            Self::Reachable { slots, .. } => slots.len(),
        }
    }

    /// Reads the value beginning at `index`; tails and unusable slots read as `None`.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&VerificationType> {
        match self {
            Self::Reachable { slots, .. } => match slots.get(index) {
                Some(Slot::Value(value)) => Some(value),
                _ => None,
            },
            Self::Bottom { .. } => None,
        }
    }

    /// Writes a local while preserving the invariant that category-2 halves never survive.
    pub fn set_local(&mut self, index: usize, value: VerificationType) -> Result<(), FrameError> {
        let Self::Reachable { kind, slots } = self else {
            return Err(FrameError::OutOfBounds);
        };
        if *kind != FrameKind::Locals {
            return Err(FrameError::WrongKind);
        }
        let width = value.width().ok_or(FrameError::OutOfBounds)?;
        if index >= slots.len() {
            return Err(FrameError::OutOfBounds);
        }
        if width == VerificationTypeWidth::Category2 && index + 1 >= slots.len() {
            return Err(FrameError::TruncatedCategory2);
        }
        let overwrote_tail = matches!(slots[index], Slot::Category2Tail);
        invalidate_value_at(slots, index);
        if index > 0 && overwrote_tail {
            invalidate_value_at(slots, index - 1);
        }
        slots[index] = Slot::Value(value);
        if width == VerificationTypeWidth::Category2 {
            invalidate_value_at(slots, index + 1);
            slots[index + 1] = Slot::Category2Tail;
        }
        Ok(())
    }

    /// Pushes one value onto an operand frame, charging its JVM category width.
    pub fn push(&mut self, value: VerificationType) -> Result<(), FrameError> {
        let Self::Reachable { kind, slots } = self else {
            return Err(FrameError::OutOfBounds);
        };
        if *kind != FrameKind::OperandStack {
            return Err(FrameError::WrongKind);
        }
        let width = match value.width() {
            Some(VerificationTypeWidth::Category1) => 1,
            Some(VerificationTypeWidth::Category2) => 2,
            None => return Err(FrameError::OutOfBounds),
        };
        let start = slots
            .iter()
            .position(|slot| matches!(slot, Slot::Unusable))
            .unwrap_or(slots.len());
        if start + width > slots.len() {
            return Err(FrameError::TruncatedCategory2);
        }
        slots[start] = Slot::Value(value);
        if width == 2 {
            slots[start + 1] = Slot::Category2Tail;
        }
        Ok(())
    }

    fn normalized_slots(&self) -> Option<&[Slot]> {
        match self {
            Self::Reachable { slots, .. } => Some(slots),
            Self::Bottom { .. } => None,
        }
    }
}

fn invalidate_value_at(slots: &mut [Slot], index: usize) {
    if matches!(slots.get(index), Some(Slot::Value(value)) if value.width() == Some(VerificationTypeWidth::Category2))
        && index + 1 < slots.len()
    {
        slots[index + 1] = Slot::Unusable;
    }
    slots[index] = Slot::Unusable;
}

#[cfg(test)]
mod environment_tests {
    use super::*;
    use crate::{ClassDefinition, ClassInitializationState, ClassLoader};
    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
    use std::{
        collections::BTreeMap,
        sync::atomic::{AtomicUsize, Ordering},
    };

    fn insert(
        cx: &Cx,
        loader: &ClassLoader,
        name: &str,
        parents: &[&str],
        methods: &[(&str, &str, u16)],
    ) {
        let metadata = JavaClassMetadata::test_class(cx, name, parents, 0, methods);
        loader.test_insert(ClassDefinition::test(
            loader.id(),
            name,
            name.len() as u64,
            metadata,
            BTreeMap::new(),
        ));
    }

    #[test]
    fn verification_environment_is_read_only_and_records_exact_lineage() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let loader = ClassLoader::new(4096);
        insert(
            &cx,
            &loader,
            "SideEffectBase",
            &[],
            &[("<clinit>", "()V", 0x0008)],
        );
        insert(
            &cx,
            &loader,
            "VerifiedChild",
            &["SideEffectBase"],
            &[("run", "()V", 0)],
        );
        insert(&cx, &loader, "Unrelated", &[], &[]);

        // These counters stand at the effect boundaries a verifier must never
        // enter. The only operation below is metadata observation; no callback
        // capable of initialization, allocation, execution, native work, or a
        // source read is supplied to the environment.
        let initializer_runs = AtomicUsize::new(0);
        let allocations = AtomicUsize::new(0);
        let executions = AtomicUsize::new(0);
        let native_calls = AtomicUsize::new(0);
        let source_reads = AtomicUsize::new(0);
        let initialization = ClassInitializationState::Uninitialized;

        let environment = VerificationEnvironment::new(&loader, 3);
        let dependency_capacity = environment.dependencies.borrow().capacity();
        assert_eq!(
            environment.is_assignable("VerifiedChild", "SideEffectBase", 2),
            Ok(VerificationAssignability::Assignable)
        );
        let child = environment.class("VerifiedChild").unwrap();
        assert_eq!(
            child.methods().map(JavaMember::name).collect::<Vec<_>>(),
            ["run"]
        );
        assert_eq!(
            environment.dependencies.borrow().capacity(),
            dependency_capacity
        );
        let dependencies = environment.dependencies();
        assert_eq!(
            dependencies
                .iter()
                .map(|dependency| dependency.class().binary_name())
                .collect::<Vec<_>>(),
            ["VerifiedChild", "SideEffectBase"]
        );
        assert!(
            dependencies
                .iter()
                .all(|dependency| dependency.revision() == loader.revision())
        );
        assert_eq!(initialization, ClassInitializationState::Uninitialized);
        assert_eq!(initializer_runs.load(Ordering::Relaxed), 0);
        assert_eq!(allocations.load(Ordering::Relaxed), 0);
        assert_eq!(executions.load(Ordering::Relaxed), 0);
        assert_eq!(native_calls.load(Ordering::Relaxed), 0);
        assert_eq!(source_reads.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn verification_environment_refuses_loading_and_bounds_every_walk() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let loader = ClassLoader::new(4096);
        insert(&cx, &loader, "Child", &["Parent"], &[]);
        insert(&cx, &loader, "Parent", &[], &[]);
        let environment = VerificationEnvironment::new(&loader, 2);

        assert_eq!(
            environment.is_assignable("Child", "Parent", 1),
            Err(VerificationQueryError::LineageLimit { limit: 1 })
        );
        assert!(matches!(
            environment.class("Missing"),
            Err(VerificationQueryError::NotLoaded(name)) if name == "Missing"
        ));
    }
}

impl StateSize for VerificationFrame {
    fn state_size(&self) -> usize {
        size_of::<Self>() + self.capacity() * size_of::<Slot>()
    }
}

impl JoinSemilattice for VerificationFrame {
    fn bottom(&self) -> Self {
        Self::bottom_frame(self.kind(), self.capacity())
    }

    fn join(&self, other: &Self) -> Self {
        if self.kind() != other.kind() || self.capacity() != other.capacity() {
            return Self::new(self.kind(), self.capacity().max(other.capacity()));
        }
        match (self.normalized_slots(), other.normalized_slots()) {
            (None, _) => other.clone(),
            (_, None) => self.clone(),
            (Some(left), Some(right)) => {
                let slots = left
                    .iter()
                    .zip(right)
                    .map(|(left, right)| match (left, right) {
                        (Slot::Value(a), Slot::Value(b)) => Slot::Value(a.join(b)),
                        (Slot::Category2Tail, Slot::Category2Tail) => Slot::Category2Tail,
                        (Slot::Unusable, Slot::Unusable) => Slot::Unusable,
                        _ => Slot::Unusable,
                    })
                    .collect::<Vec<_>>();
                let mut result = Self::Reachable {
                    kind: self.kind(),
                    slots: slots.into_boxed_slice(),
                };
                normalize_category2(&mut result);
                result
            }
        }
    }

    fn less_equal(&self, other: &Self) -> bool {
        self.join(other) == *other
    }
}

fn normalize_category2(frame: &mut VerificationFrame) {
    let VerificationFrame::Reachable { slots, .. } = frame else {
        return;
    };
    for index in 0..slots.len() {
        let valid_head = matches!(&slots[index], Slot::Value(value) if value.width() == Some(VerificationTypeWidth::Category2))
            && matches!(slots.get(index + 1), Some(Slot::Category2Tail));
        let valid_tail = index > 0
            && matches!(&slots[index - 1], Slot::Value(value) if value.width() == Some(VerificationTypeWidth::Category2));
        if (matches!(&slots[index], Slot::Value(value) if value.width() == Some(VerificationTypeWidth::Category2))
            && !valid_head)
            || (matches!(slots[index], Slot::Category2Tail) && !valid_tail)
        {
            slots[index] = Slot::Unusable;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_incremental_core::dataflow::LawSuite;

    fn types() -> Vec<VerificationType> {
        vec![
            VerificationType::Bottom,
            VerificationType::Int,
            VerificationType::Float,
            VerificationType::Long,
            VerificationType::Double,
            VerificationType::Null,
            VerificationType::Reference(ReferenceType::Object),
            VerificationType::Reference(ReferenceType::Class("java/lang/String".into())),
            VerificationType::Reference(ReferenceType::Array("[I".into())),
            VerificationType::UninitializedThis,
            VerificationType::Uninitialized(7),
            VerificationType::Uninitialized(11),
            VerificationType::Unusable,
        ]
    }

    #[test]
    fn every_verification_type_pair_and_triple_obeys_the_delivered_laws() {
        LawSuite::check_lattice(&types()).unwrap();
    }

    #[test]
    fn exhaustive_small_frames_obey_the_delivered_laws() {
        let values = types()
            .into_iter()
            .filter(|value| value.width().is_some())
            .collect::<Vec<_>>();
        let mut frames = vec![
            VerificationFrame::bottom_frame(FrameKind::Locals, 2),
            VerificationFrame::new(FrameKind::Locals, 2),
        ];
        for first in &values {
            let mut frame = VerificationFrame::new(FrameKind::Locals, 2);
            if frame.set_local(0, first.clone()).is_ok() {
                frames.push(frame);
            }
            for second in &values {
                let mut frame = VerificationFrame::new(FrameKind::Locals, 2);
                if frame.set_local(0, first.clone()).is_ok()
                    && frame.set_local(1, second.clone()).is_ok()
                {
                    frames.push(frame);
                }
            }
        }
        LawSuite::check_lattice(&frames).unwrap();
    }

    #[test]
    fn half_overwriting_category_two_local_makes_the_old_value_unusable() {
        let mut locals = VerificationFrame::new(FrameKind::Locals, 3);
        locals.set_local(0, VerificationType::Long).unwrap();
        locals.set_local(1, VerificationType::Int).unwrap();
        assert_eq!(locals.get(0), None);
        assert_eq!(locals.get(1), Some(&VerificationType::Int));
    }

    #[test]
    fn operand_frames_charge_category_widths() {
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 3);
        stack.push(VerificationType::Long).unwrap();
        stack.push(VerificationType::Int).unwrap();
        assert_eq!(stack.get(0), Some(&VerificationType::Long));
        assert_eq!(stack.get(2), Some(&VerificationType::Int));
    }
}
