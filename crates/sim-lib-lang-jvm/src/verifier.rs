//! JVM verification types and lawful dataflow frames.

use sim_codec_classfile::{
    InstructionId, Opcode, StackMapFrame, StackMapTableAttribute,
    VerificationType as ClassfileVerificationType,
};
use sim_incremental_core::dataflow::{
    Boundary, DataflowGraph, EdgeClass, EdgeSpec, GraphBuildError, GraphDirection, JoinSemilattice,
    LocatedGraphAdapter, NodeSpec, StateSize,
};
use sim_lib_machine::{LocatedCode, SourceLocation};
use std::{cell::RefCell, mem::size_of, sync::Arc};

use crate::{
    ClassDefinition, ClassDefinitionId, ClassLoader, ClassLoaderId, ClassSpaceRevision,
    JavaClassMetadata, JavaMember, JavaMemberKind, PreparedJvmPolicy,
};

/// Whether a prepared instruction can complete with a guest throwable.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ThrowCapability {
    /// The instruction has no JVM exceptional-completion path.
    Never,
    /// The instruction may complete with a guest throwable.
    MayThrow,
}

/// Located metadata retained by each verifier dataflow node.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VerificationNodeLocation {
    /// Inclusive classfile byte offset.
    pub offset: usize,
    /// Exclusive classfile byte offset.
    pub end: usize,
    /// Exceptional-completion capability used to construct handler edges.
    pub throw_capability: ThrowCapability,
}

/// JVM control transfer represented by the shared dataflow graph.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum VerificationEdgeClass {
    /// Sequential execution of the following instruction.
    Fallthrough,
    /// An explicit branch or switch target.
    Branch,
    /// Ordered transfer to one classfile exception handler.
    Exceptional {
        /// Original exception-table row, preserving handler search order.
        row: usize,
        /// Constant-pool catch class, or zero for a catch-all handler.
        catch_type: u16,
    },
}

/// Stable identity of a verifier graph edge.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VerificationEdgeId {
    source: u32,
    ordinal: usize,
}

impl VerificationEdgeId {
    /// Returns the shared instruction identity at which this edge originates.
    pub const fn source(self) -> InstructionId {
        InstructionId(self.source)
    }

    /// Returns the edge's stable declaration ordinal within its source projection.
    pub const fn ordinal(self) -> usize {
        self.ordinal
    }
}

/// The shared dataflow graph specialized with JVM verifier identities and metadata.
pub type VerificationGraph =
    DataflowGraph<u32, VerificationEdgeId, VerificationNodeLocation, VerificationEdgeClass>;

/// Located refusal while adapting JVM code to the verifier graph.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationGraphError {
    /// A non-terminating instruction would fall through the end of the method.
    IllegalFallthrough {
        /// Shared instruction identity at the illegal terminal point.
        instruction: InstructionId,
        /// Exact classfile byte offset of the instruction.
        offset: usize,
    },
    /// Legacy `jsr`/`ret` subroutines are outside the verifier policy.
    LegacySubroutine {
        /// Shared instruction identity carrying the legacy opcode.
        instruction: InstructionId,
        /// Exact classfile byte offset of the instruction.
        offset: usize,
    },
    /// The shared graph constructor rejected the projection.
    Graph(GraphBuildError<u32, VerificationEdgeId>),
}

/// Borrowing adapter from shared machine locations to the one shared dataflow graph.
struct JvmVerificationAdapter<'a> {
    code: &'a LocatedCode<PreparedJvmPolicy>,
}

impl LocatedGraphAdapter for JvmVerificationAdapter<'_> {
    type NodeId = u32;
    type EdgeId = VerificationEdgeId;
    type Location = VerificationNodeLocation;
    type Class = VerificationEdgeClass;

    fn nodes(&self) -> Vec<NodeSpec<Self::NodeId, Self::Location>> {
        cursors(self.code)
            .map(|cursor| {
                let located = self.code.instruction(cursor);
                let (offset, end) = byte_range(located.location());
                NodeSpec {
                    id: located.id().0,
                    location: VerificationNodeLocation {
                        offset,
                        end,
                        throw_capability: throw_capability(located.instruction().opcode()),
                    },
                    boundary: Boundary::Internal,
                }
            })
            .collect()
    }

    fn edges(&self) -> Vec<EdgeSpec<Self::EdgeId, Self::NodeId, Self::Class>> {
        graph_edges(self.code).expect("adapter is validated before shared graph construction")
    }
}

/// Adapts prepared JVM code and its ordered exception table to `DATAFLOW_2`.
pub fn build_verification_graph(
    code: &LocatedCode<PreparedJvmPolicy>,
) -> Result<VerificationGraph, VerificationGraphError> {
    validate_graph_policy(code)?;
    JvmVerificationAdapter { code }
        .build_graph()
        .map_err(VerificationGraphError::Graph)
}

fn validate_graph_policy(
    code: &LocatedCode<PreparedJvmPolicy>,
) -> Result<(), VerificationGraphError> {
    for cursor in cursors(code) {
        let located = code.instruction(cursor);
        let opcode = located.instruction().opcode();
        let (offset, _) = byte_range(located.location());
        if matches!(opcode, Opcode::Jsr | Opcode::JsrW | Opcode::Ret) {
            return Err(VerificationGraphError::LegacySubroutine {
                instruction: *located.id(),
                offset,
            });
        }
        let control = opcode.metadata().control;
        if code.next(cursor).is_none()
            && matches!(control, "fallthrough" | "conditional-branch" | "invoke")
        {
            return Err(VerificationGraphError::IllegalFallthrough {
                instruction: *located.id(),
                offset,
            });
        }
    }
    Ok(())
}

fn graph_edges(
    code: &LocatedCode<PreparedJvmPolicy>,
) -> Result<Vec<EdgeSpec<VerificationEdgeId, u32, VerificationEdgeClass>>, VerificationGraphError> {
    let mut edges = Vec::new();
    for cursor in cursors(code) {
        let located = code.instruction(cursor);
        let instruction = *located.id();
        let source = instruction.0;
        let control = located.instruction().opcode().metadata().control;
        let mut ordinal = 0;
        let mut push = |target, class| {
            edges.push(EdgeSpec {
                id: VerificationEdgeId { source, ordinal },
                source,
                target,
                class: EdgeClass::Custom(class),
                direction: GraphDirection::Forward,
            });
            ordinal += 1;
        };
        for target in code.branch_targets(instruction) {
            push(
                code.instruction(*target).id().0,
                VerificationEdgeClass::Branch,
            );
        }
        if matches!(control, "fallthrough" | "conditional-branch" | "invoke") {
            let next = code.next(cursor).ok_or_else(|| {
                let (offset, _) = byte_range(located.location());
                VerificationGraphError::IllegalFallthrough {
                    instruction,
                    offset,
                }
            })?;
            push(
                code.instruction(next).id().0,
                VerificationEdgeClass::Fallthrough,
            );
        }
        if throw_capability(located.instruction().opcode()) == ThrowCapability::MayThrow {
            for handler in located.instruction().handler_membership() {
                push(
                    handler.handler.0,
                    VerificationEdgeClass::Exceptional {
                        row: handler.row,
                        catch_type: handler.catch_type,
                    },
                );
            }
        }
    }
    Ok(edges)
}

fn cursors(
    code: &LocatedCode<PreparedJvmPolicy>,
) -> impl Iterator<Item = sim_lib_machine::CodeCursor> + '_ {
    std::iter::successors(Some(code.entry()), |cursor| code.next(*cursor))
}

fn byte_range(location: &SourceLocation) -> (usize, usize) {
    match location {
        SourceLocation::Bytes(origin) => (origin.span.start, origin.span.end),
        SourceLocation::Tokens { origin, .. } => (origin.span.start, origin.span.end),
    }
}

fn throw_capability(opcode: Opcode) -> ThrowCapability {
    use Opcode::*;
    if matches!(
        opcode,
        Iaload
            | Laload
            | Faload
            | Daload
            | Aaload
            | Baload
            | Caload
            | Saload
            | Iastore
            | Lastore
            | Fastore
            | Dastore
            | Aastore
            | Bastore
            | Castore
            | Sastore
            | Idiv
            | Ldiv
            | Irem
            | Lrem
            | Getstatic
            | Putstatic
            | Getfield
            | Putfield
            | Invokevirtual
            | Invokespecial
            | Invokestatic
            | Invokeinterface
            | Invokedynamic
            | New
            | Newarray
            | Anewarray
            | Arraylength
            | Athrow
            | Checkcast
            | Monitorenter
            | Monitorexit
            | Multianewarray
    ) {
        ThrowCapability::MayThrow
    } else {
        ThrowCapability::Never
    }
}

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

/// Method facts needed to derive the verifier's implicit entry frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InitialFrameInput<'a> {
    /// Internal binary name of the class declaring the method.
    pub declaring_class: &'a str,
    /// Exact JVMS method name (`<init>` identifies a constructor).
    pub method_name: &'a str,
    /// Exact JVMS method descriptor.
    pub descriptor: &'a str,
    /// Whether the method carries `ACC_STATIC`.
    pub is_static: bool,
    /// Physical local-variable slot limit from the enclosing `Code` attribute.
    pub max_locals: usize,
    /// Physical operand-stack slot limit from the enclosing `Code` attribute.
    pub max_stack: usize,
}

/// One declared stack-map frame, expanded and anchored to a decoded instruction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExpandedStackMapFrame {
    /// Exact classfile byte offset computed from the compressed deltas.
    pub offset: u32,
    /// Stable identity of the instruction beginning at `offset`.
    pub instruction: InstructionId,
    /// Complete logical local sequence (category-2 tails are implicit).
    pub locals: Box<[VerificationType]>,
    /// Complete logical operand-stack sequence (category-2 tails are implicit).
    pub stack: Box<[VerificationType]>,
}

/// A precise refusal while deriving or expanding declared verifier frames.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StackMapExpansionError {
    /// The method descriptor is malformed.
    InvalidDescriptor {
        /// Refused descriptor.
        descriptor: Box<str>,
        /// Byte offset at which parsing failed.
        offset: usize,
    },
    /// Descriptor-derived locals exceed the `Code` attribute limit.
    LocalsWidth {
        /// Declared frame offset, or `None` for the implicit entry frame.
        offset: Option<u32>,
        /// Required physical slot width.
        width: usize,
        /// Declared `max_locals` limit.
        limit: usize,
    },
    /// A declared operand stack exceeds the `Code` attribute limit.
    StackWidth {
        /// Declared frame offset.
        offset: u32,
        /// Required physical slot width.
        width: usize,
        /// Declared `max_stack` limit.
        limit: usize,
    },
    /// A compressed frame removes more logical locals than its predecessor has.
    ChopUnderflow {
        /// Declared frame offset.
        offset: u32,
        /// Number of logical locals requested for removal.
        requested: usize,
        /// Number of logical predecessor locals available.
        available: usize,
    },
    /// The encoded append count disagrees with its retained payload.
    AppendCount {
        /// Declared frame offset.
        offset: u32,
        /// Count encoded by the frame tag.
        declared: usize,
        /// Number of retained payload entries.
        actual: usize,
    },
    /// A classfile verification type names an invalid constant-pool class entry.
    InvalidClass {
        /// Declared frame offset.
        offset: u32,
        /// Refused constant-pool index.
        constant_pool_index: u16,
    },
    /// Offset-delta progression overflowed the classfile offset domain.
    OffsetOverflow,
    /// The declared offset does not begin a decoded instruction.
    NotInstructionBoundary {
        /// Exact non-boundary byte offset.
        offset: u32,
    },
}

/// Derives the exact logical locals of the implicit entry frame.
pub fn derive_initial_locals(
    input: &InitialFrameInput<'_>,
) -> Result<Vec<VerificationType>, StackMapExpansionError> {
    let mut locals = Vec::new();
    if !input.is_static {
        locals.push(if input.method_name == "<init>" {
            VerificationType::UninitializedThis
        } else {
            VerificationType::Reference(ReferenceType::Class(input.declaring_class.into()))
        });
    }
    let bytes = input.descriptor.as_bytes();
    if !input.descriptor.is_ascii() || bytes.first() != Some(&b'(') {
        return Err(descriptor_error(input.descriptor, 0));
    }
    let mut cursor = 1;
    while bytes.get(cursor) != Some(&b')') {
        if cursor >= bytes.len() {
            return Err(descriptor_error(input.descriptor, cursor));
        }
        locals.push(parse_descriptor_type(input.descriptor, &mut cursor)?);
    }
    cursor += 1;
    if cursor >= bytes.len() {
        return Err(descriptor_error(input.descriptor, cursor));
    }
    if bytes[cursor] == b'V' {
        cursor += 1;
    } else {
        let _ = parse_descriptor_type(input.descriptor, &mut cursor)?;
    }
    if cursor != bytes.len() {
        return Err(descriptor_error(input.descriptor, cursor));
    }
    let width = verification_width(&locals);
    if width > input.max_locals {
        return Err(StackMapExpansionError::LocalsWidth {
            offset: None,
            width,
            limit: input.max_locals,
        });
    }
    Ok(locals)
}

/// Expands verbatim classfile stack-map records and binds each to an instruction identity.
pub fn expand_stack_map_table<F>(
    table: &StackMapTableAttribute,
    input: &InitialFrameInput<'_>,
    code: &LocatedCode<PreparedJvmPolicy>,
    mut resolve_class: F,
) -> Result<Vec<ExpandedStackMapFrame>, StackMapExpansionError>
where
    F: FnMut(u16) -> Option<ReferenceType>,
{
    let mut locals = derive_initial_locals(input)?;
    let mut previous_offset: Option<u32> = None;
    let mut expanded = Vec::with_capacity(table.frames.len());
    for encoded in &table.frames {
        let delta = frame_offset_delta(encoded);
        let offset = match previous_offset {
            None => u32::from(delta),
            Some(previous) => previous
                .checked_add(u32::from(delta))
                .and_then(|value| value.checked_add(1))
                .ok_or(StackMapExpansionError::OffsetOverflow)?,
        };
        let mut stack = Vec::new();
        match encoded {
            StackMapFrame::Same { .. } | StackMapFrame::SameExtended { .. } => {}
            StackMapFrame::SameLocalsOneStack { stack: value, .. }
            | StackMapFrame::SameLocalsOneStackExtended { stack: value, .. } => {
                stack.push(convert_classfile_type(*value, offset, &mut resolve_class)?);
            }
            StackMapFrame::Chop { frame_type, .. } => {
                let count = usize::from(251 - *frame_type);
                if count > locals.len() {
                    return Err(StackMapExpansionError::ChopUnderflow {
                        offset,
                        requested: count,
                        available: locals.len(),
                    });
                }
                locals.truncate(locals.len() - count);
            }
            StackMapFrame::Append {
                frame_type,
                locals: appended,
                ..
            } => {
                let declared = usize::from(*frame_type - 251);
                if declared != appended.len() {
                    return Err(StackMapExpansionError::AppendCount {
                        offset,
                        declared,
                        actual: appended.len(),
                    });
                }
                for value in appended {
                    locals.push(convert_classfile_type(*value, offset, &mut resolve_class)?);
                }
            }
            StackMapFrame::Full {
                locals: complete,
                stack: complete_stack,
                ..
            } => {
                locals = complete
                    .iter()
                    .map(|value| convert_classfile_type(*value, offset, &mut resolve_class))
                    .collect::<Result<_, _>>()?;
                stack = complete_stack
                    .iter()
                    .map(|value| convert_classfile_type(*value, offset, &mut resolve_class))
                    .collect::<Result<_, _>>()?;
            }
        }
        let locals_width = verification_width(&locals);
        if locals_width > input.max_locals {
            return Err(StackMapExpansionError::LocalsWidth {
                offset: Some(offset),
                width: locals_width,
                limit: input.max_locals,
            });
        }
        let stack_width = verification_width(&stack);
        if stack_width > input.max_stack {
            return Err(StackMapExpansionError::StackWidth {
                offset,
                width: stack_width,
                limit: input.max_stack,
            });
        }
        let instruction = instruction_at_offset(code, offset)
            .ok_or(StackMapExpansionError::NotInstructionBoundary { offset })?;
        expanded.push(ExpandedStackMapFrame {
            offset,
            instruction,
            locals: locals.clone().into_boxed_slice(),
            stack: stack.into_boxed_slice(),
        });
        previous_offset = Some(offset);
    }
    Ok(expanded)
}

fn descriptor_error(descriptor: &str, offset: usize) -> StackMapExpansionError {
    StackMapExpansionError::InvalidDescriptor {
        descriptor: descriptor.into(),
        offset,
    }
}

fn parse_descriptor_type(
    descriptor: &str,
    cursor: &mut usize,
) -> Result<VerificationType, StackMapExpansionError> {
    let bytes = descriptor.as_bytes();
    let start = *cursor;
    let value = match bytes.get(*cursor).copied() {
        Some(b'B' | b'C' | b'I' | b'S' | b'Z') => VerificationType::Int,
        Some(b'F') => VerificationType::Float,
        Some(b'J') => VerificationType::Long,
        Some(b'D') => VerificationType::Double,
        Some(b'L') => {
            let end = bytes[*cursor + 1..]
                .iter()
                .position(|byte| *byte == b';')
                .map(|relative| *cursor + 1 + relative)
                .ok_or_else(|| descriptor_error(descriptor, start))?;
            if end == *cursor + 1 {
                return Err(descriptor_error(descriptor, start));
            }
            let name = &descriptor[*cursor + 1..end];
            *cursor = end;
            VerificationType::Reference(ReferenceType::Class(name.into()))
        }
        Some(b'[') => {
            while bytes.get(*cursor) == Some(&b'[') {
                *cursor += 1;
            }
            match bytes.get(*cursor).copied() {
                Some(b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z') => {}
                Some(b'L') => {
                    let end = bytes[*cursor + 1..]
                        .iter()
                        .position(|byte| *byte == b';')
                        .map(|relative| *cursor + 1 + relative)
                        .ok_or_else(|| descriptor_error(descriptor, start))?;
                    if end == *cursor + 1 {
                        return Err(descriptor_error(descriptor, start));
                    }
                    *cursor = end;
                }
                _ => return Err(descriptor_error(descriptor, start)),
            }
            VerificationType::Reference(ReferenceType::Array(descriptor[start..=*cursor].into()))
        }
        _ => return Err(descriptor_error(descriptor, start)),
    };
    *cursor += 1;
    Ok(value)
}

fn frame_offset_delta(frame: &StackMapFrame) -> u16 {
    match frame {
        StackMapFrame::Same { frame_type } => u16::from(*frame_type),
        StackMapFrame::SameLocalsOneStack { frame_type, .. } => u16::from(*frame_type - 64),
        StackMapFrame::SameLocalsOneStackExtended { offset_delta, .. }
        | StackMapFrame::Chop { offset_delta, .. }
        | StackMapFrame::SameExtended { offset_delta }
        | StackMapFrame::Append { offset_delta, .. }
        | StackMapFrame::Full { offset_delta, .. } => *offset_delta,
    }
}

fn convert_classfile_type<F>(
    value: ClassfileVerificationType,
    offset: u32,
    resolve_class: &mut F,
) -> Result<VerificationType, StackMapExpansionError>
where
    F: FnMut(u16) -> Option<ReferenceType>,
{
    Ok(match value {
        ClassfileVerificationType::Top => VerificationType::Unusable,
        ClassfileVerificationType::Integer => VerificationType::Int,
        ClassfileVerificationType::Float => VerificationType::Float,
        ClassfileVerificationType::Double => VerificationType::Double,
        ClassfileVerificationType::Long => VerificationType::Long,
        ClassfileVerificationType::Null => VerificationType::Null,
        ClassfileVerificationType::UninitializedThis => VerificationType::UninitializedThis,
        ClassfileVerificationType::Object(index) => resolve_class(index)
            .map(VerificationType::Reference)
            .ok_or(StackMapExpansionError::InvalidClass {
                offset,
                constant_pool_index: index,
            })?,
        ClassfileVerificationType::Uninitialized(new_offset) => {
            VerificationType::Uninitialized(u32::from(new_offset))
        }
    })
}

fn verification_width(values: &[VerificationType]) -> usize {
    values
        .iter()
        .map(|value| match value.width() {
            Some(VerificationTypeWidth::Category2) => 2,
            _ => 1,
        })
        .sum()
}

fn instruction_at_offset(
    code: &LocatedCode<PreparedJvmPolicy>,
    wanted: u32,
) -> Option<InstructionId> {
    cursors(code).find_map(|cursor| {
        let instruction = code.instruction(cursor);
        let (offset, _) = byte_range(instruction.location());
        (offset == wanted as usize).then_some(*instruction.id())
    })
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
    use crate::{
        JvmInstructionPolicy, JvmInstructionSemantics, JvmSlotKind, PreparationError, prepare_code,
    };
    use sim_codec_classfile::{
        ByteReader, CodeException, ConstantPool, InstructionErrorKind, Opcode, decode_instructions,
    };
    use sim_incremental_core::dataflow::{EdgeClass, LawSuite};
    use sim_kernel::SourceId;

    const NONE: &[JvmSlotKind] = &[];
    const INT: &[JvmSlotKind] = &[JvmSlotKind::CategoryOne];

    struct GraphPolicy;

    impl JvmInstructionPolicy for GraphPolicy {
        fn semantics(opcode: Opcode) -> Option<JvmInstructionSemantics> {
            let (pops, pushes) = match opcode {
                Opcode::Iconst0 => (NONE, INT),
                Opcode::Idiv => (
                    &[JvmSlotKind::CategoryOne, JvmSlotKind::CategoryOne][..],
                    INT,
                ),
                Opcode::Ireturn => (INT, NONE),
                Opcode::Return | Opcode::Goto | Opcode::Jsr | Opcode::Ret => (NONE, NONE),
                _ => return None,
            };
            Some(JvmInstructionSemantics {
                pops,
                pushes,
                safepoint: false,
            })
        }
    }

    fn empty_pool() -> ConstantPool {
        ConstantPool::decode(&mut ByteReader::new(&[0, 1], 1), 61).unwrap()
    }

    fn prepared(bytes: &[u8], handlers: &[CodeException]) -> LocatedCode<PreparedJvmPolicy> {
        let decoded = decode_instructions(bytes, 61, &empty_pool()).unwrap();
        prepare_code::<GraphPolicy>(
            &decoded,
            bytes.len(),
            handlers,
            SourceId("Verifier.graph()V".into()),
        )
        .unwrap()
    }

    #[test]
    fn graph_reuses_locations_and_only_throwing_instructions_reach_handlers() {
        let bytes = [
            Opcode::Iconst0 as u8,
            Opcode::Iconst0 as u8,
            Opcode::Idiv as u8,
            Opcode::Ireturn as u8,
        ];
        let handlers = [CodeException {
            start_pc: 0,
            end_pc: 3,
            handler_pc: 3,
            catch_type: 7,
        }];
        let code = prepared(&bytes, &handlers);
        let graph = build_verification_graph(&code).unwrap();

        assert_eq!(graph.nodes().len(), code.len());
        assert_eq!(
            graph.node(&0).unwrap().location().throw_capability,
            ThrowCapability::Never
        );
        assert_eq!(
            graph.node(&2).unwrap().location().throw_capability,
            ThrowCapability::MayThrow
        );
        let exceptional_sources = graph
            .edges()
            .filter_map(|edge| match edge.class() {
                EdgeClass::Custom(VerificationEdgeClass::Exceptional {
                    row: 0,
                    catch_type: 7,
                }) => Some(*edge.source()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(exceptional_sources, [2]);
    }

    #[test]
    fn mid_instruction_handler_is_rejected_with_its_byte_offset() {
        let bytes = [Opcode::Goto as u8, 0, 3, Opcode::Return as u8];
        let decoded = decode_instructions(&bytes, 61, &empty_pool()).unwrap();
        let error = match prepare_code::<GraphPolicy>(
            &decoded,
            bytes.len(),
            &[CodeException {
                start_pc: 1,
                end_pc: 3,
                handler_pc: 3,
                catch_type: 0,
            }],
            SourceId("Verifier.badHandler()V".into()),
        ) {
            Ok(_) => panic!("mid-instruction handler must be rejected"),
            Err(error) => error,
        };
        let PreparationError::Classfile(error) = error else {
            panic!("handler validation must retain its classfile refusal")
        };
        assert_eq!(error.kind, InstructionErrorKind::InvalidHandler);
        assert_eq!(error.offset, 1);
    }

    #[test]
    fn illegal_fallthrough_and_legacy_subroutines_are_located() {
        let fallthrough = prepared(&[Opcode::Iconst0 as u8], &[]);
        assert_eq!(
            build_verification_graph(&fallthrough).unwrap_err(),
            VerificationGraphError::IllegalFallthrough {
                instruction: InstructionId(0),
                offset: 0
            }
        );

        let legacy = prepared(&[Opcode::Jsr as u8, 0, 3, Opcode::Return as u8], &[]);
        assert_eq!(
            build_verification_graph(&legacy).unwrap_err(),
            VerificationGraphError::LegacySubroutine {
                instruction: InstructionId(0),
                offset: 0
            }
        );
    }

    fn frame_input<'a>(
        name: &'a str,
        descriptor: &'a str,
        is_static: bool,
    ) -> InitialFrameInput<'a> {
        InitialFrameInput {
            declaring_class: "sample/Owner",
            method_name: name,
            descriptor,
            is_static,
            max_locals: 8,
            max_stack: 4,
        }
    }

    #[test]
    fn initial_locals_are_exact_for_static_instance_and_constructor_descriptors() {
        assert_eq!(
            derive_initial_locals(&frame_input("work", "(IJ[Ljava/lang/String;)V", true)).unwrap(),
            [
                VerificationType::Int,
                VerificationType::Long,
                VerificationType::Reference(ReferenceType::Array("[Ljava/lang/String;".into())),
            ]
        );
        assert_eq!(
            derive_initial_locals(&frame_input("work", "(D)Ljava/lang/Object;", false)).unwrap(),
            [
                VerificationType::Reference(ReferenceType::Class("sample/Owner".into())),
                VerificationType::Double,
            ]
        );
        assert_eq!(
            derive_initial_locals(&frame_input("<init>", "()V", false)).unwrap(),
            [VerificationType::UninitializedThis]
        );
    }

    #[test]
    fn every_compressed_frame_form_expands_to_independent_expected_state() {
        use ClassfileVerificationType as C;
        let code = prepared(&[Opcode::Return as u8; 7], &[]);
        let table = StackMapTableAttribute {
            frames: vec![
                StackMapFrame::Same { frame_type: 0 },
                StackMapFrame::SameLocalsOneStack {
                    frame_type: 64,
                    stack: C::Integer,
                },
                StackMapFrame::SameLocalsOneStackExtended {
                    offset_delta: 0,
                    stack: C::Long,
                },
                StackMapFrame::Append {
                    frame_type: 252,
                    offset_delta: 0,
                    locals: vec![C::Float],
                },
                StackMapFrame::Chop {
                    frame_type: 250,
                    offset_delta: 0,
                },
                StackMapFrame::SameExtended { offset_delta: 0 },
                StackMapFrame::Full {
                    offset_delta: 0,
                    locals: vec![C::Object(7), C::Double],
                    stack: vec![C::Null, C::Uninitialized(3)],
                },
            ],
        };
        let actual = expand_stack_map_table(
            &table,
            &frame_input("work", "(J)V", false),
            &code,
            |index| (index == 7).then(|| ReferenceType::Class("java/lang/Object".into())),
        )
        .unwrap();
        let owner = VerificationType::Reference(ReferenceType::Class("sample/Owner".into()));
        let expectations = vec![
            (vec![owner.clone(), VerificationType::Long], vec![]),
            (
                vec![owner.clone(), VerificationType::Long],
                vec![VerificationType::Int],
            ),
            (
                vec![owner.clone(), VerificationType::Long],
                vec![VerificationType::Long],
            ),
            (
                vec![
                    owner.clone(),
                    VerificationType::Long,
                    VerificationType::Float,
                ],
                vec![],
            ),
            (vec![owner.clone(), VerificationType::Long], vec![]),
            (vec![owner, VerificationType::Long], vec![]),
            (
                vec![
                    VerificationType::Reference(ReferenceType::Class("java/lang/Object".into())),
                    VerificationType::Double,
                ],
                vec![VerificationType::Null, VerificationType::Uninitialized(3)],
            ),
        ];
        for (index, (frame, (locals, stack))) in actual.iter().zip(expectations).enumerate() {
            assert_eq!(frame.offset, index as u32);
            assert_eq!(frame.instruction, InstructionId(index as u32));
            assert_eq!(frame.locals.as_ref(), locals);
            assert_eq!(frame.stack.as_ref(), stack);
        }
    }

    #[test]
    fn non_boundary_stack_map_offset_is_rejected_naming_the_offset() {
        let code = prepared(&[Opcode::Goto as u8, 0, 3, Opcode::Return as u8], &[]);
        let error = expand_stack_map_table(
            &StackMapTableAttribute {
                frames: vec![StackMapFrame::Same { frame_type: 1 }],
            },
            &frame_input("work", "()V", true),
            &code,
            |_| None,
        )
        .unwrap_err();
        assert_eq!(
            error,
            StackMapExpansionError::NotInstructionBoundary { offset: 1 }
        );
    }

    #[test]
    fn expanded_frames_enforce_physical_local_and_stack_widths() {
        let code = prepared(&[Opcode::Return as u8], &[]);
        let mut input = frame_input("work", "()V", true);
        input.max_locals = 1;
        input.max_stack = 1;
        let locals_error = expand_stack_map_table(
            &StackMapTableAttribute {
                frames: vec![StackMapFrame::Full {
                    offset_delta: 0,
                    locals: vec![ClassfileVerificationType::Long],
                    stack: vec![],
                }],
            },
            &input,
            &code,
            |_| None,
        )
        .unwrap_err();
        assert_eq!(
            locals_error,
            StackMapExpansionError::LocalsWidth {
                offset: Some(0),
                width: 2,
                limit: 1,
            }
        );

        let stack_error = expand_stack_map_table(
            &StackMapTableAttribute {
                frames: vec![StackMapFrame::SameLocalsOneStack {
                    frame_type: 64,
                    stack: ClassfileVerificationType::Double,
                }],
            },
            &input,
            &code,
            |_| None,
        )
        .unwrap_err();
        assert_eq!(
            stack_error,
            StackMapExpansionError::StackWidth {
                offset: 0,
                width: 2,
                limit: 1,
            }
        );
    }

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
