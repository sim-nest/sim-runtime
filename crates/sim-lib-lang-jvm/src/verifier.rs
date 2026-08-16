//! JVM verification types and lawful dataflow frames.

use sim_codec_classfile::{
    InstructionId, InstructionOperand, Opcode, StackMapFrame, StackMapTableAttribute,
    VerificationType as ClassfileVerificationType,
};
use sim_incremental_core::dataflow::{
    Boundary, DataflowGraph, EdgeClass, EdgeSpec, GraphBuildError, GraphDirection, JoinSemilattice,
    LocatedGraphAdapter, NodeSpec, StateSize,
};
use sim_lib_machine::{LocatedCode, SourceLocation};
use std::{cell::RefCell, collections::BTreeMap, mem::size_of, sync::Arc};

use crate::{
    ClassDefinition, ClassDefinitionId, ClassLoader, ClassLoaderId, ClassSpaceRevision,
    JavaClassMetadata, JavaMember, JavaMemberKind, PreparedJvmPolicy,
};

/// Resolves the verification type of a loadable constant-pool entry.
pub trait VerificationConstantResolver {
    /// Returns the type denoted by `index`, or `None` for an invalid constant.
    fn verification_type(&self, index: u16) -> Option<VerificationType>;
}

impl<F> VerificationConstantResolver for F
where
    F: Fn(u16) -> Option<VerificationType>,
{
    fn verification_type(&self, index: u16) -> Option<VerificationType> {
        self(index)
    }
}

/// The single generated verifier owner for an opcode identity.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum VerifierRuleFamily {
    /// Constants, local access, array access, and operand-stack manipulation.
    ConstantsLocalsStack,
    /// Numeric arithmetic, comparison, and conversion.
    NumericConversion,
    /// Branches, switches, and method exits.
    ControlReturn,
    /// Fields, objects, arrays, invocation, and monitors.
    ObjectArrayField,
    /// An opcode deliberately outside the admitted verifier policy.
    ExplicitRefusal,
}

/// Generated dispatch record binding one shared opcode identity to one owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct VerifierRule {
    /// Identity from the sole classfile opcode manifest.
    pub opcode: Opcode,
    /// Rule family responsible for verification or explicit refusal.
    pub family: VerifierRuleFamily,
}

include!("verifier_rules_generated.rs");

/// Returns the generated verifier dispatch record for `opcode`.
#[must_use]
pub const fn verifier_rule(opcode: Opcode) -> &'static VerifierRule {
    &VERIFIER_RULES[opcode as usize]
}

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
    /// A malformed array component descriptor was supplied.
    InvalidDescriptor(String),
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

/// Normative reason for a verifier reference join.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationJoinRule {
    /// One input is assignable to the other (JVMS 4.10.1.2).
    AssignableInput,
    /// The least loaded common superclass (JVMS 4.10.1.2).
    CommonSuperclass,
    /// Unrelated interface types merge to `java/lang/Object` (JVMS 4.10.1.2).
    UnrelatedInterfaces,
    /// Array covariance recursively joined the reference component types.
    ArrayComponents,
}

/// Dependency and work evidence returned by every bounded verifier query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationQueryEvidence {
    /// Loaded class identities consulted by this query, in observation order.
    pub dependencies: Vec<ClassDefinitionId>,
    /// Caller-provided hierarchy-node budget.
    pub node_limit: usize,
    /// Number of hierarchy nodes charged by the query.
    pub nodes_used: usize,
}

/// A successful bounded verifier query and its proof evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationQuery<T> {
    /// Normative query result.
    pub value: T,
    /// Exact dependencies and budget consumption.
    pub evidence: VerificationQueryEvidence,
}

/// A refused verifier query with the same dependency and budget evidence as a success.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationQueryFailure {
    /// Exact reason the query could not be answered normatively.
    pub error: VerificationQueryError,
    /// Dependencies and work consumed before refusal.
    pub evidence: VerificationQueryEvidence,
}

/// Result of joining two verification types.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationTypeJoin {
    /// Joined lattice value.
    pub value: VerificationType,
    /// Reference rule used when the join required one.
    pub rule: Option<VerificationJoinRule>,
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

    /// Checks verifier reference assignability using JVMS 4.10.1.2 rules.
    pub fn reference_assignability(
        &self,
        actual: &ReferenceType,
        expected: &ReferenceType,
        node_limit: usize,
    ) -> Result<VerificationQuery<VerificationAssignability>, VerificationQueryFailure> {
        let start = self.dependencies.borrow().len();
        let mut remaining = node_limit;
        let answer = self.reference_reaches(actual, expected, node_limit, &mut remaining);
        let value =
            if answer.map_err(|error| self.query_failure(error, start, node_limit, remaining))? {
                VerificationAssignability::Assignable
            } else {
                VerificationAssignability::NotAssignable
            };
        Ok(VerificationQuery {
            value,
            evidence: self.query_evidence(start, node_limit, remaining),
        })
    }

    /// Joins two verifier values without resolving missing hierarchy metadata.
    pub fn join_types(
        &self,
        left: &VerificationType,
        right: &VerificationType,
        node_limit: usize,
    ) -> Result<VerificationQuery<VerificationTypeJoin>, VerificationQueryFailure> {
        let start = self.dependencies.borrow().len();
        let mut remaining = node_limit;
        let joined = self
            .join_types_inner(left, right, node_limit, &mut remaining)
            .map_err(|error| self.query_failure(error, start, node_limit, remaining))?;
        Ok(VerificationQuery {
            value: joined,
            evidence: self.query_evidence(start, node_limit, remaining),
        })
    }

    fn query_evidence(
        &self,
        start: usize,
        limit: usize,
        remaining: usize,
    ) -> VerificationQueryEvidence {
        VerificationQueryEvidence {
            dependencies: self.dependencies.borrow()[start..]
                .iter()
                .map(|d| d.class().clone())
                .collect(),
            node_limit: limit,
            nodes_used: limit - remaining,
        }
    }

    fn query_failure(
        &self,
        error: VerificationQueryError,
        start: usize,
        limit: usize,
        remaining: usize,
    ) -> VerificationQueryFailure {
        VerificationQueryFailure {
            error,
            evidence: self.query_evidence(start, limit, remaining),
        }
    }

    fn reference_reaches(
        &self,
        actual: &ReferenceType,
        expected: &ReferenceType,
        limit: usize,
        remaining: &mut usize,
    ) -> Result<bool, VerificationQueryError> {
        if actual == expected || matches!(expected, ReferenceType::Object) {
            return Ok(true);
        }
        match (actual, expected) {
            (ReferenceType::Class(actual), ReferenceType::Class(expected)) => {
                self.lineage_reaches(actual, expected, limit, remaining)
            }
            (ReferenceType::Array(_), ReferenceType::Class(expected))
                if matches!(
                    expected.as_ref(),
                    "java/lang/Cloneable" | "java/io/Serializable"
                ) =>
            {
                Ok(true)
            }
            (ReferenceType::Array(actual), ReferenceType::Array(expected)) => {
                self.array_assignable(actual, expected, limit, remaining)
            }
            _ => Ok(false),
        }
    }

    fn array_assignable(
        &self,
        actual: &str,
        expected: &str,
        limit: usize,
        remaining: &mut usize,
    ) -> Result<bool, VerificationQueryError> {
        let (Some(a), Some(e)) = (actual.strip_prefix('['), expected.strip_prefix('[')) else {
            return Ok(false);
        };
        if is_primitive_descriptor(a) || is_primitive_descriptor(e) {
            return Ok(a == e);
        }
        self.reference_reaches(
            &descriptor_reference(a)?,
            &descriptor_reference(e)?,
            limit,
            remaining,
        )
    }

    fn join_types_inner(
        &self,
        left: &VerificationType,
        right: &VerificationType,
        limit: usize,
        remaining: &mut usize,
    ) -> Result<VerificationTypeJoin, VerificationQueryError> {
        use VerificationType::{Bottom, Null, Reference, Unusable};
        let plain = |value| VerificationTypeJoin { value, rule: None };
        match (left, right) {
            (Bottom, value) | (value, Bottom) => Ok(plain(value.clone())),
            (Unusable, _) | (_, Unusable) => Ok(plain(Unusable)),
            (a, b) if a == b => Ok(plain(a.clone())),
            (Null, Reference(r)) | (Reference(r), Null) => Ok(plain(Reference(r.clone()))),
            (Reference(a), Reference(b)) => self.join_references(a, b, limit, remaining),
            _ => Ok(plain(Unusable)),
        }
    }

    fn join_references(
        &self,
        left: &ReferenceType,
        right: &ReferenceType,
        limit: usize,
        remaining: &mut usize,
    ) -> Result<VerificationTypeJoin, VerificationQueryError> {
        let result = |r, rule| VerificationTypeJoin {
            value: VerificationType::Reference(r),
            rule: Some(rule),
        };
        if self.reference_reaches(left, right, limit, remaining)? {
            return Ok(result(right.clone(), VerificationJoinRule::AssignableInput));
        }
        if self.reference_reaches(right, left, limit, remaining)? {
            return Ok(result(left.clone(), VerificationJoinRule::AssignableInput));
        }
        if let (ReferenceType::Array(a), ReferenceType::Array(b)) = (left, right) {
            let (Some(ac), Some(bc)) = (a.strip_prefix('['), b.strip_prefix('[')) else {
                unreachable!()
            };
            if !is_primitive_descriptor(ac) && !is_primitive_descriptor(bc) {
                let joined = self.join_references(
                    &descriptor_reference(ac)?,
                    &descriptor_reference(bc)?,
                    limit,
                    remaining,
                )?;
                if let VerificationType::Reference(reference) = joined.value {
                    return Ok(result(
                        ReferenceType::Array(
                            format!("[{}", reference_descriptor(&reference)).into_boxed_str(),
                        ),
                        VerificationJoinRule::ArrayComponents,
                    ));
                }
            }
        }
        let (ReferenceType::Class(a), ReferenceType::Class(b)) = (left, right) else {
            return Ok(result(
                ReferenceType::Object,
                VerificationJoinRule::CommonSuperclass,
            ));
        };
        let ac = self.observe(a)?;
        let bc = self.observe(b)?;
        if ac.is_interface() && bc.is_interface() {
            return Ok(result(
                ReferenceType::Object,
                VerificationJoinRule::UnrelatedInterfaces,
            ));
        }
        let mut current = a.to_string();
        loop {
            if self.lineage_reaches(b, &current, limit, remaining)? {
                return Ok(result(
                    ReferenceType::Class(current.clone().into_boxed_str()),
                    VerificationJoinRule::CommonSuperclass,
                ));
            }
            if *remaining == 0 {
                return Err(VerificationQueryError::LineageLimit { limit });
            }
            *remaining -= 1;
            let class = self.observe(&current)?;
            let Some(parent) = class.metadata().resolution().direct_parents().first() else {
                break;
            };
            current.clone_from(parent);
        }
        Ok(result(
            ReferenceType::Object,
            VerificationJoinRule::CommonSuperclass,
        ))
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

fn is_primitive_descriptor(descriptor: &str) -> bool {
    descriptor.len() == 1
        && matches!(
            descriptor.as_bytes()[0],
            b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z'
        )
}

fn descriptor_reference(descriptor: &str) -> Result<ReferenceType, VerificationQueryError> {
    if descriptor.starts_with('[') {
        Ok(ReferenceType::Array(descriptor.into()))
    } else if let Some(name) = descriptor
        .strip_prefix('L')
        .and_then(|d| d.strip_suffix(';'))
    {
        Ok(ReferenceType::Class(name.into()))
    } else {
        Err(VerificationQueryError::InvalidDescriptor(
            descriptor.to_owned(),
        ))
    }
}

fn reference_descriptor(reference: &ReferenceType) -> String {
    match reference {
        ReferenceType::Object => "Ljava/lang/Object;".to_owned(),
        ReferenceType::Class(name) => format!("L{name};"),
        ReferenceType::Array(descriptor) => descriptor.to_string(),
    }
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

/// Typed locals and operand stack at one verifier program point.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationState {
    /// Fixed-size local-variable frame.
    pub locals: VerificationFrame,
    /// Fixed-size operand-stack frame.
    pub stack: VerificationFrame,
}

impl VerificationState {
    /// Builds the reachable method-entry state, including receiver and arguments.
    pub fn initial(input: &InitialFrameInput<'_>) -> Result<Self, StackMapExpansionError> {
        let values = derive_initial_locals(input)?;
        let mut locals = VerificationFrame::new(FrameKind::Locals, input.max_locals);
        let mut slot = 0;
        for value in values {
            let width = type_width(&value);
            locals
                .set_local(slot, value)
                .expect("derive_initial_locals already checked the physical bound");
            slot += width;
        }
        Ok(Self {
            locals,
            stack: VerificationFrame::new(FrameKind::OperandStack, input.max_stack),
        })
    }
}

/// Precise reason a constant/local/stack instruction was refused.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationTransferKind {
    /// The prepared operand shape is inconsistent with the opcode.
    MalformedPreparedInput,
    /// A local index or category-2 extent exceeds `max_locals`.
    LocalBounds,
    /// The operand stack underflows or exceeds `max_stack`.
    StackBounds,
    /// A local or stack value has the wrong computational category.
    Category,
    /// A method exit is incompatible with the method descriptor's return type.
    ReturnType,
    /// A constant-pool entry is absent or has the wrong width for its opcode.
    Constant {
        /// Refused constant-pool index.
        index: u16,
    },
}

/// Descriptor-derived return category used by the control verifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum VerificationReturnType {
    /// The method returns no value.
    Void,
    /// The method returns this verification type.
    Value(VerificationType),
}

/// Applies one branch, switch, or return rule without selecting an edge.
///
/// Edge selection remains the graph's concern: both conditional successors receive the same
/// post-pop state, switches propagate to every declared target, and returns propagate nowhere.
pub fn transfer_control_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
    return_type: &VerificationReturnType,
) -> Result<VerificationState, VerificationTransferError> {
    use Opcode::*;
    let opcode = instruction.opcode();
    let operands = instruction.instruction().operands.as_slice();
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode,
        kind,
    };
    let expected = match opcode {
        Ifeq | Ifne | Iflt | Ifge | Ifgt | Ifle | Tableswitch | Lookupswitch => {
            &[VerificationType::Int][..]
        }
        IfIcmpeq | IfIcmpne | IfIcmplt | IfIcmpge | IfIcmpgt | IfIcmple => {
            &[VerificationType::Int, VerificationType::Int][..]
        }
        IfAcmpeq | IfAcmpne => &[
            VerificationType::Reference(ReferenceType::Object),
            VerificationType::Reference(ReferenceType::Object),
        ][..],
        Ifnull | Ifnonnull => &[VerificationType::Reference(ReferenceType::Object)][..],
        Goto | GotoW => &[][..],
        Ireturn => &[VerificationType::Int][..],
        Lreturn => &[VerificationType::Long][..],
        Freturn => &[VerificationType::Float][..],
        Dreturn => &[VerificationType::Double][..],
        Areturn => &[VerificationType::Reference(ReferenceType::Object)][..],
        Return => &[][..],
        _ => return Err(fail(VerificationTransferKind::MalformedPreparedInput)),
    };
    if !control_operands_valid(opcode, operands) {
        return Err(fail(VerificationTransferKind::MalformedPreparedInput));
    }
    let values = stack_values(&state.stack);
    if values.len() < expected.len() {
        return Err(fail(VerificationTransferKind::StackBounds));
    }
    let split = values.len() - expected.len();
    if !values[split..]
        .iter()
        .zip(expected)
        .all(|(actual, wanted)| verification_category_matches(actual, wanted))
    {
        return Err(fail(VerificationTransferKind::Category));
    }
    let actual_return = match opcode {
        Ireturn | Lreturn | Freturn | Dreturn | Areturn => {
            Some(values.last().expect("return input was checked"))
        }
        Return => None,
        _ => {
            let mut next = state.clone();
            next.stack = stack_from_values(state.stack.capacity(), values[..split].to_vec())
                .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
            return Ok(next);
        }
    };
    let compatible = match (actual_return, return_type) {
        (None, VerificationReturnType::Void) => true,
        (Some(actual), VerificationReturnType::Value(declared)) => actual.less_equal(declared),
        _ => false,
    };
    if !compatible {
        return Err(fail(VerificationTransferKind::ReturnType));
    }
    let mut next = state.clone();
    next.stack = stack_from_values(state.stack.capacity(), values[..split].to_vec())
        .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
    Ok(next)
}

fn verification_category_matches(actual: &VerificationType, wanted: &VerificationType) -> bool {
    match wanted {
        VerificationType::Reference(_) => matches!(
            actual,
            VerificationType::Null | VerificationType::Reference(_)
        ),
        _ => actual == wanted,
    }
}

fn control_operands_valid(opcode: Opcode, operands: &[InstructionOperand]) -> bool {
    use Opcode::*;
    match opcode {
        Ifeq | Ifne | Iflt | Ifge | Ifgt | Ifle | IfIcmpeq | IfIcmpne | IfIcmplt | IfIcmpge
        | IfIcmpgt | IfIcmple | IfAcmpeq | IfAcmpne | Goto | Ifnull | Ifnonnull | GotoW => {
            matches!(operands, [InstructionOperand::Branch(_)])
        }
        Tableswitch => matches!(
            operands,
            [InstructionOperand::Branch(_), InstructionOperand::TableLow(low), InstructionOperand::TableHigh(high), rest @ ..]
            if i64::from(*high) - i64::from(*low) + 1 == rest.len() as i64
                && rest.iter().all(|operand| matches!(operand, InstructionOperand::Branch(_)))
        ),
        Lookupswitch => {
            matches!(operands.split_first(), Some((InstructionOperand::Branch(_), rest))
            if rest.len() % 2 == 0 && rest.chunks_exact(2).all(|pair| matches!(pair,
                [InstructionOperand::LookupKey(_), InstructionOperand::Branch(_)])))
        }
        Ireturn | Lreturn | Freturn | Dreturn | Areturn | Return => operands.is_empty(),
        _ => false,
    }
}

/// Refusal while checking joined dataflow states against declared target frames.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StackMapConstraintError {
    /// A classfile version requiring target frames omitted one.
    Missing { instruction: InstructionId },
    /// A declaration has a different shape or is not a supertype of the inferred state.
    NotAssignable { instruction: InstructionId },
    /// An inferred target state was unavailable after dataflow completed.
    MissingInference { instruction: InstructionId },
}

/// Checks target declarations after the shared engine has joined all incoming states.
pub fn check_stack_map_constraints(
    classfile_version: u16,
    graph: &VerificationGraph,
    inferred: &BTreeMap<InstructionId, VerificationState>,
    declarations: &[ExpandedStackMapFrame],
    max_locals: usize,
    max_stack: usize,
) -> Result<(), StackMapConstraintError> {
    let declared: BTreeMap<_, _> = declarations
        .iter()
        .map(|frame| (frame.instruction, frame))
        .collect();
    let targets: std::collections::BTreeSet<_> = graph
        .edges()
        .filter(|edge| {
            matches!(
                edge.class(),
                EdgeClass::Custom(
                    VerificationEdgeClass::Branch | VerificationEdgeClass::Exceptional { .. }
                )
            )
        })
        .map(|edge| InstructionId(*edge.target()))
        .collect();
    for instruction in targets {
        let Some(frame) = declared.get(&instruction) else {
            if classfile_version >= 51 {
                return Err(StackMapConstraintError::Missing { instruction });
            }
            continue;
        };
        let state = inferred
            .get(&instruction)
            .ok_or(StackMapConstraintError::MissingInference { instruction })?;
        let declared_state = expanded_state(frame, max_locals, max_stack)
            .ok_or(StackMapConstraintError::NotAssignable { instruction })?;
        if !state.locals.less_equal(&declared_state.locals)
            || !state.stack.less_equal(&declared_state.stack)
            || stack_values(&state.stack).len() != stack_values(&declared_state.stack).len()
        {
            return Err(StackMapConstraintError::NotAssignable { instruction });
        }
    }
    Ok(())
}

fn expanded_state(
    frame: &ExpandedStackMapFrame,
    max_locals: usize,
    max_stack: usize,
) -> Option<VerificationState> {
    let mut locals = VerificationFrame::new(FrameKind::Locals, max_locals);
    let mut slot = 0;
    for value in &*frame.locals {
        locals.set_local(slot, value.clone()).ok()?;
        slot += type_width(value);
    }
    Some(VerificationState {
        locals,
        stack: stack_from_values(max_stack, frame.stack.to_vec()).ok()?,
    })
}

/// Located refusal from the constants, locals, and stack transfer family.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationTransferError {
    /// Shared instruction identity at the refusal.
    pub instruction: InstructionId,
    /// Exact classfile byte offset at the refusal.
    pub offset: usize,
    /// Opcode being checked.
    pub opcode: Opcode,
    /// Stable refusal classification.
    pub kind: VerificationTransferKind,
}

/// Applies one numeric arithmetic, bitwise, shift, comparison, or conversion rule atomically.
pub(crate) fn transfer_numeric_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
) -> Result<VerificationState, VerificationTransferError> {
    let opcode = instruction.opcode();
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode,
        kind,
    };
    if opcode == Opcode::Iinc {
        return transfer_storage_instruction(instruction, offset, state, &|_| None);
    }
    if !instruction.instruction().operands.is_empty() {
        return Err(fail(VerificationTransferKind::MalformedPreparedInput));
    }
    let (inputs, output): (&[VerificationType], VerificationType) = numeric_signature(opcode)
        .ok_or_else(|| fail(VerificationTransferKind::MalformedPreparedInput))?;
    let mut values = stack_values(&state.stack);
    if values.len() < inputs.len() {
        return Err(fail(VerificationTransferKind::StackBounds));
    }
    let split = values.len() - inputs.len();
    if values[split..] != *inputs {
        return Err(fail(VerificationTransferKind::Category));
    }
    values.truncate(split);
    values.push(output);
    let mut next = state.clone();
    next.stack = stack_from_values(state.stack.capacity(), values)
        .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
    Ok(next)
}

fn numeric_signature(opcode: Opcode) -> Option<(&'static [VerificationType], VerificationType)> {
    use Opcode::*;
    use VerificationType::{Double, Float, Int, Long};
    const II: &[VerificationType] = &[Int, Int];
    const LL: &[VerificationType] = &[Long, Long];
    const FF: &[VerificationType] = &[Float, Float];
    const DD: &[VerificationType] = &[Double, Double];
    const LI: &[VerificationType] = &[Long, Int];
    const I: &[VerificationType] = &[Int];
    const L: &[VerificationType] = &[Long];
    const F: &[VerificationType] = &[Float];
    const D: &[VerificationType] = &[Double];
    Some(match opcode {
        Iadd | Isub | Imul | Idiv | Irem | Iand | Ior | Ixor | Ishl | Ishr | Iushr => (II, Int),
        Ladd | Lsub | Lmul | Ldiv | Lrem | Land | Lor | Lxor => (LL, Long),
        Lshl | Lshr | Lushr => (LI, Long),
        Fadd | Fsub | Fmul | Fdiv | Frem => (FF, Float),
        Dadd | Dsub | Dmul | Ddiv | Drem => (DD, Double),
        Ineg | I2b | I2c | I2s => (I, Int),
        Lneg => (L, Long),
        Fneg => (F, Float),
        Dneg => (D, Double),
        I2l => (I, Long),
        I2f => (I, Float),
        I2d => (I, Double),
        L2i => (L, Int),
        L2f => (L, Float),
        L2d => (L, Double),
        F2i => (F, Int),
        F2l => (F, Long),
        F2d => (F, Double),
        D2i => (D, Int),
        D2l => (D, Long),
        D2f => (D, Float),
        Lcmp => (LL, Int),
        Fcmpl | Fcmpg => (FF, Int),
        Dcmpl | Dcmpg => (DD, Int),
        _ => return None,
    })
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

    /// Performs a fallible, hierarchy-aware frame join.
    pub fn join_with_environment(
        &self,
        other: &Self,
        environment: &VerificationEnvironment<'_>,
        node_limit: usize,
    ) -> Result<VerificationQuery<Self>, VerificationQueryFailure> {
        let start = environment.dependencies.borrow().len();
        let mut remaining = node_limit;
        if self.kind() != other.kind() || self.capacity() != other.capacity() {
            return Ok(VerificationQuery {
                value: Self::new(self.kind(), self.capacity().max(other.capacity())),
                evidence: environment.query_evidence(start, node_limit, remaining),
            });
        }
        let value = match (self.normalized_slots(), other.normalized_slots()) {
            (None, _) => other.clone(),
            (_, None) => self.clone(),
            (Some(left), Some(right)) => {
                let mut slots = Vec::with_capacity(left.len());
                for (a, b) in left.iter().zip(right) {
                    slots.push(match (a, b) {
                        (Slot::Value(a), Slot::Value(b)) => Slot::Value(
                            environment
                                .join_types_inner(a, b, node_limit, &mut remaining)
                                .map_err(|error| {
                                    environment.query_failure(error, start, node_limit, remaining)
                                })?
                                .value,
                        ),
                        (Slot::Category2Tail, Slot::Category2Tail) => Slot::Category2Tail,
                        (Slot::Unusable, Slot::Unusable) => Slot::Unusable,
                        _ => Slot::Unusable,
                    });
                }
                let mut frame = Self::Reachable {
                    kind: self.kind(),
                    slots: slots.into_boxed_slice(),
                };
                normalize_category2(&mut frame);
                frame
            }
        };
        Ok(VerificationQuery {
            value,
            evidence: environment.query_evidence(start, node_limit, remaining),
        })
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

/// Applies one constants, locals, or stack transfer rule atomically.
pub fn transfer_storage_instruction<R: VerificationConstantResolver>(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
    constants: &R,
) -> Result<VerificationState, VerificationTransferError> {
    let opcode = instruction.opcode();
    let operands = instruction.instruction().operands.as_slice();
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode,
        kind,
    };
    let mut next = state.clone();
    if opcode == Opcode::Nop {
        return operands
            .is_empty()
            .then_some(next)
            .ok_or_else(|| fail(VerificationTransferKind::MalformedPreparedInput));
    }
    let constant = match opcode {
        Opcode::AconstNull => Some(VerificationType::Null),
        Opcode::IconstM1
        | Opcode::Iconst0
        | Opcode::Iconst1
        | Opcode::Iconst2
        | Opcode::Iconst3
        | Opcode::Iconst4
        | Opcode::Iconst5
        | Opcode::Bipush
        | Opcode::Sipush => Some(VerificationType::Int),
        Opcode::Lconst0 | Opcode::Lconst1 => Some(VerificationType::Long),
        Opcode::Fconst0 | Opcode::Fconst1 | Opcode::Fconst2 => Some(VerificationType::Float),
        Opcode::Dconst0 | Opcode::Dconst1 => Some(VerificationType::Double),
        Opcode::Ldc | Opcode::LdcW | Opcode::Ldc2W => {
            let [InstructionOperand::Constant(index)] = operands else {
                return Err(fail(VerificationTransferKind::MalformedPreparedInput));
            };
            let value = constants
                .verification_type(*index)
                .ok_or_else(|| fail(VerificationTransferKind::Constant { index: *index }))?;
            let wanted = if opcode == Opcode::Ldc2W { 2 } else { 1 };
            if type_width(&value) != wanted {
                return Err(fail(VerificationTransferKind::Constant { index: *index }));
            }
            Some(value)
        }
        _ => None,
    };
    if let Some(value) = constant {
        if !constant_operands_valid(opcode, operands) {
            return Err(fail(VerificationTransferKind::MalformedPreparedInput));
        }
        next.stack
            .push(value)
            .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
        return Ok(next);
    }
    if let Some((slot, expected)) =
        verifier_local_access(opcode, operands, true).map_err(|kind| fail(kind))?
    {
        let value = next
            .locals
            .get(slot)
            .cloned()
            .ok_or_else(|| fail(VerificationTransferKind::LocalBounds))?;
        require_verification_category(&value, expected)
            .then_some(())
            .ok_or_else(|| fail(VerificationTransferKind::Category))?;
        next.stack
            .push(value)
            .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
        return Ok(next);
    }
    if let Some((slot, expected)) =
        verifier_local_access(opcode, operands, false).map_err(|kind| fail(kind))?
    {
        let mut values = stack_values(&next.stack);
        let value = values
            .pop()
            .ok_or_else(|| fail(VerificationTransferKind::StackBounds))?;
        require_verification_category(&value, expected)
            .then_some(())
            .ok_or_else(|| fail(VerificationTransferKind::Category))?;
        next.locals
            .set_local(slot, value)
            .map_err(|_| fail(VerificationTransferKind::LocalBounds))?;
        next.stack = stack_from_values(next.stack.capacity(), values)
            .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
        return Ok(next);
    }
    if opcode == Opcode::Iinc {
        let [
            InstructionOperand::Local(slot),
            InstructionOperand::Immediate(_),
        ] = operands
        else {
            return Err(fail(VerificationTransferKind::MalformedPreparedInput));
        };
        let slot = usize::from(*slot);
        let value = next
            .locals
            .get(slot)
            .ok_or_else(|| fail(VerificationTransferKind::LocalBounds))?;
        if value != &VerificationType::Int {
            return Err(fail(VerificationTransferKind::Category));
        }
        return Ok(next);
    }
    if let Some(choices) = crate::execution::shuffle_descriptor(opcode) {
        if !operands.is_empty() {
            return Err(fail(VerificationTransferKind::MalformedPreparedInput));
        }
        let values = stack_values(&next.stack);
        let widths: Vec<_> = values.iter().map(type_width).collect();
        let Some((input, output)) = choices.iter().find(|(input, _)| widths.ends_with(input))
        else {
            return Err(fail(
                if widths.iter().sum::<usize>() < instruction.input_width() {
                    VerificationTransferKind::StackBounds
                } else {
                    VerificationTransferKind::Category
                },
            ));
        };
        let prefix = values.len() - input.len();
        let mut shuffled = values[..prefix].to_vec();
        shuffled.extend(output.iter().map(|index| values[prefix + index].clone()));
        next.stack = stack_from_values(next.stack.capacity(), shuffled)
            .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
        return Ok(next);
    }
    Err(fail(VerificationTransferKind::MalformedPreparedInput))
}

#[derive(Clone, Copy)]
enum VerificationCategory {
    Int,
    Long,
    Float,
    Double,
    Reference,
}

fn type_width(value: &VerificationType) -> usize {
    usize::from(value.width() == Some(VerificationTypeWidth::Category2)) + 1
}

fn stack_values(frame: &VerificationFrame) -> Vec<VerificationType> {
    frame
        .normalized_slots()
        .into_iter()
        .flatten()
        .filter_map(|slot| match slot {
            Slot::Value(value) => Some(value.clone()),
            Slot::Unusable | Slot::Category2Tail => None,
        })
        .collect()
}

fn stack_from_values(
    capacity: usize,
    values: Vec<VerificationType>,
) -> Result<VerificationFrame, FrameError> {
    let mut frame = VerificationFrame::new(FrameKind::OperandStack, capacity);
    for value in values {
        frame.push(value)?;
    }
    Ok(frame)
}

fn constant_operands_valid(opcode: Opcode, operands: &[InstructionOperand]) -> bool {
    match opcode {
        Opcode::Bipush | Opcode::Sipush => matches!(operands, [InstructionOperand::Immediate(_)]),
        Opcode::Ldc | Opcode::LdcW | Opcode::Ldc2W => {
            matches!(operands, [InstructionOperand::Constant(_)])
        }
        _ => operands.is_empty(),
    }
}

fn require_verification_category(value: &VerificationType, expected: VerificationCategory) -> bool {
    matches!(
        (expected, value),
        (VerificationCategory::Int, VerificationType::Int)
            | (VerificationCategory::Long, VerificationType::Long)
            | (VerificationCategory::Float, VerificationType::Float)
            | (VerificationCategory::Double, VerificationType::Double)
            | (
                VerificationCategory::Reference,
                VerificationType::Null
                    | VerificationType::Reference(_)
                    | VerificationType::UninitializedThis
                    | VerificationType::Uninitialized(_)
            )
    )
}

fn verifier_local_access(
    opcode: Opcode,
    operands: &[InstructionOperand],
    load: bool,
) -> Result<Option<(usize, VerificationCategory)>, VerificationTransferKind> {
    use Opcode::*;
    let (category, fixed) = match (load, opcode) {
        (true, Iload) | (false, Istore) => (VerificationCategory::Int, None),
        (true, Lload) | (false, Lstore) => (VerificationCategory::Long, None),
        (true, Fload) | (false, Fstore) => (VerificationCategory::Float, None),
        (true, Dload) | (false, Dstore) => (VerificationCategory::Double, None),
        (true, Aload) | (false, Astore) => (VerificationCategory::Reference, None),
        _ => match verifier_implicit_local(opcode, load) {
            Some(value) => value,
            None => return Ok(None),
        },
    };
    let slot = match fixed {
        Some(slot) if operands.is_empty() => slot,
        Some(_) => return Err(VerificationTransferKind::MalformedPreparedInput),
        None => match operands {
            [InstructionOperand::Local(slot)] => usize::from(*slot),
            _ => return Err(VerificationTransferKind::MalformedPreparedInput),
        },
    };
    Ok(Some((slot, category)))
}

fn verifier_implicit_local(
    opcode: Opcode,
    load: bool,
) -> Option<(VerificationCategory, Option<usize>)> {
    use Opcode::*;
    let families = if load {
        [
            (Iload0, VerificationCategory::Int),
            (Lload0, VerificationCategory::Long),
            (Fload0, VerificationCategory::Float),
            (Dload0, VerificationCategory::Double),
            (Aload0, VerificationCategory::Reference),
        ]
    } else {
        [
            (Istore0, VerificationCategory::Int),
            (Lstore0, VerificationCategory::Long),
            (Fstore0, VerificationCategory::Float),
            (Dstore0, VerificationCategory::Double),
            (Astore0, VerificationCategory::Reference),
        ]
    };
    families.into_iter().find_map(|(first, category)| {
        let opcode = opcode as u8 as usize;
        let first = first as u8 as usize;
        (opcode >= first && opcode - first < 4).then_some((category, Some(opcode - first)))
    })
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
        insert_with_flags(cx, loader, name, parents, 0, methods);
    }

    fn insert_with_flags(
        cx: &Cx,
        loader: &ClassLoader,
        name: &str,
        parents: &[&str],
        flags: u16,
        methods: &[(&str, &str, u16)],
    ) {
        let metadata = JavaClassMetadata::test_class(cx, name, parents, flags, methods);
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

    #[test]
    fn assignability_and_join_apply_bounded_jvms_reference_rules() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let loader = ClassLoader::new(4096);
        insert_with_flags(&cx, &loader, "Left", &[], 0x0200, &[]);
        insert_with_flags(&cx, &loader, "Right", &[], 0x0200, &[]);
        insert(&cx, &loader, "Parent", &[], &[]);
        insert(&cx, &loader, "Child", &["Parent"], &[]);
        let environment = VerificationEnvironment::new(&loader, 16);

        let array = environment
            .reference_assignability(
                &ReferenceType::Array("[LChild;".into()),
                &ReferenceType::Array("[LParent;".into()),
                4,
            )
            .unwrap();
        assert_eq!(array.value, VerificationAssignability::Assignable);
        assert!(array.evidence.nodes_used <= array.evidence.node_limit);

        let joined = environment
            .join_types(
                &VerificationType::Reference(ReferenceType::Class("Left".into())),
                &VerificationType::Reference(ReferenceType::Class("Right".into())),
                4,
            )
            .unwrap();
        assert_eq!(
            joined.value,
            VerificationTypeJoin {
                value: VerificationType::Reference(ReferenceType::Object),
                rule: Some(VerificationJoinRule::UnrelatedInterfaces),
            }
        );
    }

    #[test]
    fn join_refuses_unresolved_hierarchy_and_exhausts_hostile_depth() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let loader = ClassLoader::new(4096);
        insert(&cx, &loader, "Broken", &["Missing"], &[]);
        insert(&cx, &loader, "Other", &[], &[]);
        let environment = VerificationEnvironment::new(&loader, 16);
        assert!(matches!(
            environment.join_types(
                &VerificationType::Reference(ReferenceType::Class("Broken".into())),
                &VerificationType::Reference(ReferenceType::Class("Other".into())),
                8,
            ),
            Err(VerificationQueryFailure { error: VerificationQueryError::NotLoaded(name), .. }) if name == "Missing"
        ));

        insert(&cx, &loader, "Deep0", &["Deep1"], &[]);
        insert(&cx, &loader, "Deep1", &["Deep2"], &[]);
        insert(&cx, &loader, "Deep2", &[], &[]);
        let failure = environment
            .reference_assignability(
                &ReferenceType::Class("Deep0".into()),
                &ReferenceType::Class("Other".into()),
                2,
            )
            .unwrap_err();
        assert_eq!(
            failure.error,
            VerificationQueryError::LineageLimit { limit: 2 }
        );
        assert_eq!(failure.evidence.node_limit, 2);
        assert_eq!(failure.evidence.nodes_used, 2);
        assert_eq!(failure.evidence.dependencies.len(), 2);
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

    #[test]
    fn generated_verifier_rules_cover_every_shared_opcode_once() {
        assert_eq!(VERIFIER_RULES.len(), sim_codec_classfile::OPCODES.len());
        for (byte, (rule, metadata)) in VERIFIER_RULES
            .iter()
            .zip(sim_codec_classfile::OPCODES.iter())
            .enumerate()
        {
            assert_eq!(rule.opcode, metadata.opcode, "opcode byte {byte:#04x}");
            assert_eq!(verifier_rule(metadata.opcode), rule);
        }
        for opcode in [
            Opcode::Jsr,
            Opcode::Ret,
            Opcode::JsrW,
            Opcode::Breakpoint,
            Opcode::ReservedCB,
            Opcode::Impdep1,
            Opcode::Impdep2,
        ] {
            assert_eq!(
                verifier_rule(opcode).family,
                VerifierRuleFamily::ExplicitRefusal,
                "{opcode:?} must be refused explicitly"
            );
        }
    }
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
                Opcode::Ifeq => (INT, NONE),
                Opcode::Return | Opcode::Goto | Opcode::Jsr | Opcode::Ret => (NONE, NONE),
                _ if verifier_rule(opcode).family == VerifierRuleFamily::ConstantsLocalsStack => {
                    (NONE, NONE)
                }
                _ if verifier_rule(opcode).family == VerifierRuleFamily::NumericConversion => {
                    (NONE, NONE)
                }
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

    fn storage_transfer(
        bytes: &[u8],
        locals: VerificationFrame,
        stack_values: &[VerificationType],
        stack_capacity: usize,
    ) -> Result<VerificationState, VerificationTransferError> {
        let pool = if matches!(bytes.first(), Some(opcode) if *opcode == Opcode::Ldc as u8) {
            ConstantPool::decode(&mut ByteReader::new(&[0, 2, 3, 0, 0, 0, 42], 7), 61).unwrap()
        } else {
            empty_pool()
        };
        let decoded = decode_instructions(bytes, 61, &pool).unwrap();
        let code = prepare_code::<GraphPolicy>(
            &decoded,
            bytes.len(),
            &[],
            SourceId("Verifier.storage()V".into()),
        )
        .unwrap();
        let instruction = code.instruction(code.entry());
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, stack_capacity);
        for value in stack_values {
            stack.push(value.clone()).unwrap();
        }
        transfer_storage_instruction(
            instruction.instruction(),
            0,
            &VerificationState { locals, stack },
            &|index| (index == 1).then_some(VerificationType::Int),
        )
    }

    #[test]
    fn constants_locals_stores_and_iinc_preserve_typed_bounds() {
        let input = frame_input("work", "(I)V", true);
        let initial = VerificationState::initial(&input).unwrap();
        assert_eq!(initial.locals.get(0), Some(&VerificationType::Int));

        let loaded =
            storage_transfer(&[Opcode::Iload0 as u8], initial.locals.clone(), &[], 2).unwrap();
        assert_eq!(stack_values(&loaded.stack), [VerificationType::Int]);
        let incremented =
            storage_transfer(&[Opcode::Iinc as u8, 0, 7], loaded.locals.clone(), &[], 2).unwrap();
        assert_eq!(incremented.locals.get(0), Some(&VerificationType::Int));

        let stored = storage_transfer(
            &[Opcode::Lstore as u8, 6],
            VerificationFrame::new(FrameKind::Locals, 8),
            &[VerificationType::Long],
            2,
        )
        .unwrap();
        assert_eq!(stored.locals.get(6), Some(&VerificationType::Long));
        let error = storage_transfer(
            &[Opcode::Lstore as u8, 7],
            VerificationFrame::new(FrameKind::Locals, 8),
            &[VerificationType::Long],
            2,
        )
        .unwrap_err();
        assert_eq!(error.instruction, InstructionId(0));
        assert_eq!(error.offset, 0);
        assert_eq!(error.kind, VerificationTransferKind::LocalBounds);

        let pushed = storage_transfer(
            &[Opcode::Ldc as u8, 1],
            VerificationFrame::new(FrameKind::Locals, 0),
            &[],
            1,
        )
        .unwrap();
        assert_eq!(stack_values(&pushed.stack), [VerificationType::Int]);
    }

    #[test]
    fn every_shuffle_form_uses_the_executor_descriptor() {
        use VerificationType::{Double, Float, Int, Long};
        let cases: &[(&[u8], &[VerificationType], &[VerificationType])] = &[
            (&[Opcode::Pop as u8], &[Int], &[]),
            (&[Opcode::Pop2 as u8], &[Long], &[]),
            (&[Opcode::Dup as u8], &[Int], &[Int, Int]),
            (&[Opcode::DupX1 as u8], &[Int, Float], &[Float, Int, Float]),
            (&[Opcode::DupX2 as u8], &[Long, Int], &[Int, Long, Int]),
            (&[Opcode::Dup2 as u8], &[Long], &[Long, Long]),
            (&[Opcode::Dup2X1 as u8], &[Int, Long], &[Long, Int, Long]),
            (
                &[Opcode::Dup2X2 as u8],
                &[Double, Long],
                &[Long, Double, Long],
            ),
            (&[Opcode::Swap as u8], &[Int, Float], &[Float, Int]),
        ];
        for (bytes, input, expected) in cases {
            let state = storage_transfer(
                bytes,
                VerificationFrame::new(FrameKind::Locals, 0),
                input,
                8,
            )
            .unwrap();
            assert_eq!(stack_values(&state.stack), *expected);
        }
    }

    fn numeric_transfer(
        opcode: Opcode,
        input: &[VerificationType],
    ) -> Result<VerificationState, VerificationTransferError> {
        let decoded = decode_instructions(&[opcode as u8], 61, &empty_pool()).unwrap();
        let code =
            prepare_code::<GraphPolicy>(&decoded, 1, &[], SourceId("Verifier.numeric()V".into()))
                .unwrap();
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 8);
        stack.push(VerificationType::Float).unwrap();
        for value in input {
            stack.push(value.clone()).unwrap();
        }
        transfer_numeric_instruction(
            code.instruction(code.entry()).instruction(),
            0,
            &VerificationState {
                locals: VerificationFrame::new(FrameKind::Locals, 0),
                stack,
            },
        )
    }

    #[test]
    fn every_numeric_opcode_has_exact_passing_and_failing_frames() {
        use VerificationType::{Double, Float, Int, Long};
        let mut covered = Vec::new();
        for rule in VERIFIER_RULES
            .iter()
            .filter(|rule| rule.family == VerifierRuleFamily::NumericConversion)
        {
            if rule.opcode == Opcode::Iinc {
                continue;
            }
            let (input, output) = numeric_signature(rule.opcode)
                .unwrap_or_else(|| panic!("missing numeric signature for {:?}", rule.opcode));
            let passed = numeric_transfer(rule.opcode, input).unwrap();
            assert_eq!(stack_values(&passed.stack), [Float, output.clone()]);

            let mut wrong = input.to_vec();
            let last = wrong
                .last_mut()
                .expect("every numeric rule consumes a value");
            *last = match last {
                Int => Long,
                Long | Float | Double => Int,
                other => panic!("unexpected numeric input {other:?}"),
            };
            let error = numeric_transfer(rule.opcode, &wrong).unwrap_err();
            assert_eq!(error.opcode, rule.opcode);
            assert_eq!(error.kind, VerificationTransferKind::Category);
            covered.push(rule.opcode);
        }
        assert_eq!(covered.len(), 56);

        let mut locals = VerificationFrame::new(FrameKind::Locals, 1);
        locals.set_local(0, Int).unwrap();
        let decoded = decode_instructions(&[Opcode::Iinc as u8, 0, 1], 61, &empty_pool()).unwrap();
        let code =
            prepare_code::<GraphPolicy>(&decoded, 3, &[], SourceId("Verifier.iinc()V".into()))
                .unwrap();
        let instruction = code.instruction(code.entry()).instruction();
        let state = VerificationState {
            locals: locals.clone(),
            stack: VerificationFrame::new(FrameKind::OperandStack, 0),
        };
        assert_eq!(
            transfer_numeric_instruction(instruction, 0, &state).unwrap(),
            state
        );
        locals.set_local(0, Float).unwrap();
        let error = transfer_numeric_instruction(
            instruction,
            0,
            &VerificationState {
                locals,
                stack: state.stack,
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, VerificationTransferKind::Category);
    }

    #[test]
    fn long_shift_requires_an_int_count_and_preserves_category_two_layout() {
        let shifted = numeric_transfer(
            Opcode::Lshl,
            &[VerificationType::Long, VerificationType::Int],
        )
        .unwrap();
        assert_eq!(
            shifted.stack.normalized_slots().unwrap(),
            &[
                Slot::Value(VerificationType::Float),
                Slot::Value(VerificationType::Long),
                Slot::Category2Tail,
                Slot::Unusable,
                Slot::Unusable,
                Slot::Unusable,
                Slot::Unusable,
                Slot::Unusable,
            ]
        );
        assert_eq!(
            numeric_transfer(
                Opcode::Lshl,
                &[VerificationType::Long, VerificationType::Long]
            )
            .unwrap_err()
            .kind,
            VerificationTransferKind::Category
        );
    }

    #[test]
    fn dup_x1_rejects_a_category_two_split_at_the_instruction_origin() {
        let error = storage_transfer(
            &[Opcode::DupX1 as u8],
            VerificationFrame::new(FrameKind::Locals, 0),
            &[VerificationType::Long, VerificationType::Int],
            4,
        )
        .unwrap_err();
        assert_eq!(error.instruction, InstructionId(0));
        assert_eq!(error.offset, 0);
        assert_eq!(error.opcode, Opcode::DupX1);
        assert_eq!(error.kind, VerificationTransferKind::Category);
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

    #[test]
    fn conditional_branches_pop_once_for_both_successors_and_returns_match_the_method() {
        let branch = prepared(
            &[
                Opcode::Iconst0 as u8,
                Opcode::Ifeq as u8,
                0,
                4,
                Opcode::Return as u8,
                Opcode::Return as u8,
            ],
            &[],
        );
        let instruction = branch.instruction(branch.next(branch.entry()).unwrap());
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 1);
        stack.push(VerificationType::Int).unwrap();
        let state = VerificationState {
            locals: VerificationFrame::new(FrameKind::Locals, 0),
            stack,
        };
        let next = transfer_control_instruction(
            instruction.instruction(),
            1,
            &state,
            &VerificationReturnType::Void,
        )
        .unwrap();
        assert!(stack_values(&next.stack).is_empty());

        let returning = prepared(&[Opcode::Ireturn as u8], &[]);
        transfer_control_instruction(
            returning.instruction(returning.entry()).instruction(),
            0,
            &state,
            &VerificationReturnType::Value(VerificationType::Int),
        )
        .unwrap();
        let error = transfer_control_instruction(
            returning.instruction(returning.entry()).instruction(),
            0,
            &state,
            &VerificationReturnType::Void,
        )
        .unwrap_err();
        assert_eq!(error.kind, VerificationTransferKind::ReturnType);
    }

    #[test]
    fn joined_targets_require_assignable_declared_frames_for_modern_classfiles() {
        let code = prepared(
            &[
                Opcode::Iconst0 as u8,
                Opcode::Ifeq as u8,
                0,
                4,
                Opcode::Return as u8,
                Opcode::Return as u8,
            ],
            &[],
        );
        let graph = build_verification_graph(&code).unwrap();
        let target = InstructionId(3);
        let state = VerificationState {
            locals: VerificationFrame::new(FrameKind::Locals, 0),
            stack: VerificationFrame::new(FrameKind::OperandStack, 1),
        };
        let inferred = BTreeMap::from([(target, state)]);
        let correct = ExpandedStackMapFrame {
            offset: 5,
            instruction: target,
            locals: Box::new([]),
            stack: Box::new([]),
        };
        check_stack_map_constraints(61, &graph, &inferred, &[correct.clone()], 0, 1).unwrap();

        let wider = ExpandedStackMapFrame {
            stack: Box::new([VerificationType::Int]),
            ..correct
        };
        assert_eq!(
            check_stack_map_constraints(61, &graph, &inferred, &[wider], 0, 1),
            Err(StackMapConstraintError::NotAssignable {
                instruction: target
            })
        );
        assert_eq!(
            check_stack_map_constraints(61, &graph, &inferred, &[], 0, 1),
            Err(StackMapConstraintError::Missing {
                instruction: target
            })
        );
    }
}
