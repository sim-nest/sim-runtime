//! JVM verification types and lawful dataflow frames.

use crate::VerifierCoverage;
use sim_codec_classfile::{
    InstructionId, InstructionOperand, Opcode, StackMapFrame, StackMapTableAttribute,
    VerificationType as ClassfileVerificationType,
};
use sim_incremental_core::QueryBudgets;
use sim_incremental_core::dataflow::{
    AdmittedTransfer, Boundary, CompletionProofMismatch, DataflowCompletionProof, DataflowGraph,
    EdgeClass, EdgeSpec, FixpointEngine, GraphBuildError, GraphDirection, JoinSemilattice,
    LocatedGraphAdapter, NodeSpec, StateSize, TransferPolicy,
};
use sim_incremental_core::{FingerprintValue, Observation, Revision, ValueFingerprint};
use sim_lib_machine::{LocatedCode, SourceLocation};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    mem::size_of,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use crate::{
    ClassDefinition, ClassDefinitionId, ClassLoader, ClassLoaderId, ClassSpaceRevision,
    DynamicBootstrap, DynamicLinkError, JavaClassMetadata, JavaMember, JavaMemberKind, JvmEdge,
    JvmGraphError, JvmHeap, JvmRole, PreparedJvmPolicy, STRING_CONCAT_BOOTSTRAP_DESCRIPTOR,
    STRING_CONCAT_BOOTSTRAP_NAME, STRING_CONCAT_BOOTSTRAP_OWNER,
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
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
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
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
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
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct VerificationState {
    /// Fixed-size local-variable frame.
    pub locals: VerificationFrame,
    /// Fixed-size operand-stack frame.
    pub stack: VerificationFrame,
}

impl StateSize for VerificationState {
    fn state_size(&self) -> usize {
        size_of::<Self>() + self.locals.state_size() + self.stack.state_size()
    }
}

impl JoinSemilattice for VerificationState {
    fn bottom(&self) -> Self {
        Self {
            locals: self.locals.bottom(),
            stack: self.stack.bottom(),
        }
    }

    fn join(&self, other: &Self) -> Self {
        Self {
            locals: self.locals.join(&other.locals),
            stack: self.stack.join(&other.stack),
        }
    }

    fn less_equal(&self, other: &Self) -> bool {
        self.locals.less_equal(&other.locals) && self.stack.less_equal(&other.stack)
    }
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
    /// An allocated reference was used before its constructor completed.
    UninitializedUse,
    /// A constructor invocation did not name `<init>` or did not consume an uninitialized receiver.
    IllegalConstructorReceiver,
    /// Incompatible initialized and uninitialized aliases met at a control-flow join.
    InitializationMerge,
    /// An exceptional edge would expose an uninitialized alias at handler entry.
    UninitializedHandlerEntry,
    /// The resolved member's staticness does not match the field opcode.
    FieldStaticness,
    /// The resolved member is not accessible under JVMS 5.4.4.
    MemberAccess,
    /// A protected instance member violates the receiver constraint in JVMS 4.10.1.8.
    ProtectedMemberAccess,
    /// A field descriptor or array component is incompatible with the operand type.
    MemoryType,
    /// An array opcode was applied to a non-array or to the wrong primitive array kind.
    ArrayType,
    /// A method descriptor, argument, receiver, or result is incompatible with the site.
    InvocationType,
    /// The invocation instruction disagrees with the resolved member's staticness.
    InvocationStaticness,
    /// The symbolic owner kind disagrees with the class/interface invocation instruction.
    InvocationOwnerKind,
    /// Signature-polymorphic invocation is outside the admitted JVM profile.
    SignaturePolymorphic,
    /// An `invokedynamic` bootstrap is outside the executor's admitted protocol registry.
    DynamicBootstrap(DynamicLinkError),
}

/// Resolution-only facts for one ordinary invocation instruction.
#[derive(Clone, Debug)]
pub struct VerificationInvocation<'a> {
    /// Internal name of the symbolic method owner.
    pub owner: &'a str,
    /// Whether the symbolic owner carries `ACC_INTERFACE`.
    pub owner_is_interface: bool,
    /// Resolved declaration; verification never selects or executes its body.
    pub method: &'a JavaMember,
    /// Whether ordinary member-access checks admit the declaration.
    pub accessible: bool,
    /// Whether resolution classified the declaration as signature-polymorphic.
    pub signature_polymorphic: bool,
}

/// Resolution-only facts for one `invokedynamic` instruction.
#[derive(Clone, Debug)]
pub struct VerificationDynamicInvocation<'a> {
    /// Bootstrap identity retained verbatim for fail-closed diagnostics.
    pub bootstrap: &'a DynamicBootstrap,
    /// Invoked method descriptor from the dynamic constant-pool entry.
    pub descriptor: &'a str,
}

/// Applies an ordinary invocation's descriptor and receiver rules without linkage or selection.
pub fn transfer_invocation_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
    invocation: &VerificationInvocation<'_>,
    environment: &VerificationEnvironment<'_>,
    lineage_limit: usize,
) -> Result<VerificationState, VerificationTransferError> {
    use Opcode::*;
    let opcode = instruction.opcode();
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode,
        kind,
    };
    if !matches!(
        opcode,
        Invokevirtual | Invokespecial | Invokestatic | Invokeinterface
    ) || !matches!(
        instruction.instruction().operands.first(),
        Some(InstructionOperand::Constant(_))
    ) {
        return Err(fail(VerificationTransferKind::MalformedPreparedInput));
    }
    if invocation.signature_polymorphic {
        return Err(fail(VerificationTransferKind::SignaturePolymorphic));
    }
    if !invocation.accessible {
        return Err(fail(VerificationTransferKind::MemberAccess));
    }
    let wants_static = opcode == Invokestatic;
    if invocation.method.is_static() != wants_static {
        return Err(fail(VerificationTransferKind::InvocationStaticness));
    }
    if (opcode == Invokeinterface) != invocation.owner_is_interface {
        return Err(fail(VerificationTransferKind::InvocationOwnerKind));
    }
    let (arguments, result) = method_descriptor(invocation.method.descriptor())
        .ok_or_else(|| fail(VerificationTransferKind::InvocationType))?;
    transfer_invocation_values(
        state,
        &arguments,
        result,
        (!wants_static).then_some((invocation.owner, environment, lineage_limit)),
    )
    .map_err(|kind| fail(kind))
}

/// Applies an admitted dynamic site's descriptor without consulting or mutating linker state.
pub fn transfer_dynamic_invocation_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
    invocation: &VerificationDynamicInvocation<'_>,
) -> Result<VerificationState, VerificationTransferError> {
    let opcode = instruction.opcode();
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode,
        kind,
    };
    if opcode != Opcode::Invokedynamic
        || !matches!(
            instruction.instruction().operands.first(),
            Some(InstructionOperand::Constant(_))
        )
    {
        return Err(fail(VerificationTransferKind::MalformedPreparedInput));
    }
    let bootstrap = invocation.bootstrap;
    if bootstrap.owner != STRING_CONCAT_BOOTSTRAP_OWNER
        || bootstrap.name != STRING_CONCAT_BOOTSTRAP_NAME
        || bootstrap.descriptor != STRING_CONCAT_BOOTSTRAP_DESCRIPTOR
    {
        return Err(fail(VerificationTransferKind::DynamicBootstrap(
            DynamicLinkError::UnadmittedBootstrap {
                owner: bootstrap.owner.clone(),
                name: bootstrap.name.clone(),
                descriptor: bootstrap.descriptor.clone(),
            },
        )));
    }
    let (arguments, result) = method_descriptor(invocation.descriptor)
        .ok_or_else(|| fail(VerificationTransferKind::InvocationType))?;
    transfer_invocation_values(state, &arguments, result, None).map_err(|kind| fail(kind))
}

fn transfer_invocation_values(
    state: &VerificationState,
    arguments: &[VerificationType],
    result: Option<VerificationType>,
    receiver: Option<(&str, &VerificationEnvironment<'_>, usize)>,
) -> Result<VerificationState, VerificationTransferKind> {
    let mut values = stack_values(&state.stack);
    let consumed = arguments.len() + usize::from(receiver.is_some());
    if values.len() < consumed {
        return Err(VerificationTransferKind::StackBounds);
    }
    let base = values.len() - consumed;
    let argument_base = base + usize::from(receiver.is_some());
    if !values[argument_base..]
        .iter()
        .zip(arguments)
        .all(|(actual, expected)| verification_category_matches(actual, expected))
    {
        return Err(VerificationTransferKind::InvocationType);
    }
    if let Some((owner, environment, lineage_limit)) = receiver {
        match &values[base] {
            VerificationType::Null => {}
            VerificationType::Reference(actual)
                if environment
                    .reference_assignability(
                        actual,
                        &ReferenceType::Class(owner.into()),
                        lineage_limit,
                    )
                    .is_ok_and(|answer| answer.value == VerificationAssignability::Assignable) => {}
            _ => return Err(VerificationTransferKind::InvocationType),
        }
    }
    values.truncate(base);
    if let Some(result) = result {
        values.push(result);
    }
    let mut next = state.clone();
    next.stack = stack_from_values(state.stack.capacity(), values)
        .map_err(|_| VerificationTransferKind::StackBounds)?;
    Ok(next)
}

fn method_descriptor(
    descriptor: &str,
) -> Option<(Vec<VerificationType>, Option<VerificationType>)> {
    let arguments = descriptor_arguments(descriptor)?;
    let close = descriptor.find(')')?;
    let result = &descriptor[close + 1..];
    if result == "V" {
        return Some((arguments, None));
    }
    Some((arguments, Some(descriptor_verification_type(result)?)))
}

/// Resolution facts consumed by the object/array/field verifier family.
///
/// These values contain metadata only. Building them through [`VerificationEnvironment`]
/// preserves verification's no-loading and no-initialization boundary.
#[derive(Clone, Debug)]
pub struct VerificationField<'a> {
    /// Binary name of the class that declared the resolved field.
    pub declaring: &'a str,
    /// Resolved field declaration.
    pub field: &'a JavaMember,
    /// Whether JVMS 5.4.4 permits the caller to access the declaration.
    pub accessible: bool,
    /// Whether the caller is a subclass of the declaring class.
    pub caller_is_subclass: bool,
    /// Binary name of the class containing the method being verified.
    pub caller: &'a str,
}

/// Applies fields, arrays, casts, type tests, null checks, and monitor rules.
pub fn transfer_memory_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
    field: Option<&VerificationField<'_>>,
) -> Result<VerificationState, VerificationTransferError> {
    use Opcode::*;
    let opcode = instruction.opcode();
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode,
        kind,
    };
    let mut values = stack_values(&state.stack);
    let pop = |values: &mut Vec<VerificationType>| {
        values
            .pop()
            .ok_or_else(|| fail(VerificationTransferKind::StackBounds))
    };
    let reference = |value: &VerificationType| {
        matches!(
            value,
            VerificationType::Null | VerificationType::Reference(_)
        )
    };
    match opcode {
        Getstatic | Putstatic | Getfield | Putfield => {
            let resolved =
                field.ok_or_else(|| fail(VerificationTransferKind::MalformedPreparedInput))?;
            let wants_static = matches!(opcode, Getstatic | Putstatic);
            if resolved.field.is_static() != wants_static {
                return Err(fail(VerificationTransferKind::FieldStaticness));
            }
            if !resolved.accessible {
                return Err(fail(VerificationTransferKind::MemberAccess));
            }
            let ty = descriptor_verification_type(resolved.field.descriptor())
                .ok_or_else(|| fail(VerificationTransferKind::MemoryType))?;
            if matches!(opcode, Putstatic | Putfield) {
                let actual = pop(&mut values)?;
                if !verification_category_matches(&actual, &ty) {
                    return Err(fail(VerificationTransferKind::MemoryType));
                }
            }
            if matches!(opcode, Getfield | Putfield) {
                let receiver = pop(&mut values)?;
                if !reference(&receiver) {
                    return Err(fail(VerificationTransferKind::MemoryType));
                }
                if resolved.field.access_flags() & 0x0004 != 0
                    && resolved.caller_is_subclass
                    && resolved.caller != resolved.declaring
                    && !matches!(&receiver, VerificationType::Null)
                    && !matches!(&receiver, VerificationType::Reference(ReferenceType::Class(name)) if name.as_ref() == resolved.caller)
                {
                    return Err(fail(VerificationTransferKind::ProtectedMemberAccess));
                }
            }
            if matches!(opcode, Getstatic | Getfield) {
                values.push(ty);
            }
        }
        Aaload => {
            if pop(&mut values)? != VerificationType::Int {
                return Err(fail(VerificationTransferKind::MemoryType));
            }
            let receiver = pop(&mut values)?;
            let component = array_component(&receiver)
                .ok_or_else(|| fail(VerificationTransferKind::ArrayType))?;
            if component.is_empty() {
                values.push(VerificationType::Reference(ReferenceType::Object));
            } else if is_primitive_descriptor(component) {
                return Err(fail(VerificationTransferKind::ArrayType));
            } else {
                values.push(
                    descriptor_reference(component)
                        .map(VerificationType::Reference)
                        .map_err(|_| fail(VerificationTransferKind::ArrayType))?,
                );
            }
        }
        Arraylength => {
            if array_component(&pop(&mut values)?).is_none() {
                return Err(fail(VerificationTransferKind::ArrayType));
            }
            values.push(VerificationType::Int);
        }
        Checkcast | Instanceof => {
            if !reference(&pop(&mut values)?) {
                return Err(fail(VerificationTransferKind::MemoryType));
            }
            values.push(if opcode == Instanceof {
                VerificationType::Int
            } else {
                VerificationType::Reference(ReferenceType::Object)
            });
        }
        Ifnull | Ifnonnull | Monitorenter | Monitorexit => {
            if !reference(&pop(&mut values)?) {
                return Err(fail(VerificationTransferKind::MemoryType));
            }
        }
        Newarray | Anewarray | Multianewarray => {
            let dimensions = if opcode == Multianewarray { 1 } else { 1 };
            for _ in 0..dimensions {
                if pop(&mut values)? != VerificationType::Int {
                    return Err(fail(VerificationTransferKind::MemoryType));
                }
            }
            values.push(VerificationType::Reference(ReferenceType::Array(
                "[Ljava/lang/Object;".into(),
            )));
        }
        _ => return Err(fail(VerificationTransferKind::MalformedPreparedInput)),
    }
    let mut next = state.clone();
    next.stack = stack_from_values(state.stack.capacity(), values)
        .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
    Ok(next)
}

fn descriptor_verification_type(descriptor: &str) -> Option<VerificationType> {
    let mut cursor = 0;
    let ty = parse_descriptor_type(descriptor, &mut cursor).ok()?;
    (cursor == descriptor.len()).then_some(ty)
}

fn array_component(value: &VerificationType) -> Option<&str> {
    match value {
        VerificationType::Null => Some(""),
        VerificationType::Reference(ReferenceType::Array(descriptor)) => {
            descriptor.strip_prefix('[')
        }
        _ => None,
    }
}

/// Resolved symbolic facts for one constructor invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerificationConstructor {
    /// Internal class name of the constructed object.
    pub owner: Box<str>,
    /// Exact JVM member name; legal initialization requires `<init>`.
    pub name: Box<str>,
    /// Exact JVM method descriptor.
    pub descriptor: Box<str>,
    /// Exact allocation-site type legally consumed by this resolved constructor.
    pub receiver: VerificationType,
}

/// Applies `new`, retaining the prepared instruction identity as the allocation-site type.
pub fn transfer_new_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
) -> Result<VerificationState, VerificationTransferError> {
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode: instruction.opcode(),
        kind,
    };
    if instruction.opcode() != Opcode::New
        || !matches!(
            instruction.instruction().operands.as_slice(),
            [InstructionOperand::Constant(_)]
        )
    {
        return Err(fail(VerificationTransferKind::MalformedPreparedInput));
    }
    let mut next = state.clone();
    next.stack
        .push(VerificationType::Uninitialized(instruction.id().0))
        .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
    Ok(next)
}

/// Applies `invokespecial <init>`, replacing every frame alias after successful initialization.
pub fn transfer_constructor_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
    constructor: &VerificationConstructor,
) -> Result<VerificationState, VerificationTransferError> {
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode: instruction.opcode(),
        kind,
    };
    if instruction.opcode() != Opcode::Invokespecial
        || !matches!(
            instruction.instruction().operands.as_slice(),
            [InstructionOperand::Constant(_)]
        )
    {
        return Err(fail(VerificationTransferKind::MalformedPreparedInput));
    }
    if constructor.name.as_ref() != "<init>" {
        return Err(fail(VerificationTransferKind::IllegalConstructorReceiver));
    }
    let arguments = descriptor_arguments(&constructor.descriptor)
        .ok_or_else(|| fail(VerificationTransferKind::MalformedPreparedInput))?;
    let mut values = stack_values(&state.stack);
    if values.len() < arguments.len() + 1 {
        return Err(fail(VerificationTransferKind::StackBounds));
    }
    let receiver_index = values.len() - arguments.len() - 1;
    if !values[receiver_index + 1..]
        .iter()
        .zip(&arguments)
        .all(|(actual, expected)| verification_category_matches(actual, expected))
    {
        return Err(fail(VerificationTransferKind::Category));
    }
    let receiver = values[receiver_index].clone();
    if receiver != constructor.receiver
        || !matches!(
            receiver,
            VerificationType::Uninitialized(_) | VerificationType::UninitializedThis
        )
    {
        return Err(fail(VerificationTransferKind::IllegalConstructorReceiver));
    }
    let initialized = VerificationType::Reference(ReferenceType::Class(constructor.owner.clone()));
    values.truncate(receiver_index);
    for value in &mut values {
        if *value == receiver {
            *value = initialized.clone();
        }
    }
    let mut next = state.clone();
    replace_alias(&mut next.locals, &receiver, &initialized);
    replace_alias(&mut next.stack, &receiver, &initialized);
    next.stack = stack_from_values(next.stack.capacity(), values)
        .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
    Ok(next)
}

/// Rejects a control-flow merge that combines initialized state with a live allocation alias.
pub fn join_initialization_states(
    instruction: InstructionId,
    offset: usize,
    left: &VerificationState,
    right: &VerificationState,
) -> Result<VerificationState, VerificationTransferError> {
    let fail = || VerificationTransferError {
        instruction,
        offset,
        opcode: Opcode::Nop,
        kind: VerificationTransferKind::InitializationMerge,
    };
    reject_initialization_conflict(&left.locals, &right.locals)
        .then_some(())
        .ok_or_else(fail)?;
    reject_initialization_conflict(&left.stack, &right.stack)
        .then_some(())
        .ok_or_else(fail)?;
    Ok(VerificationState {
        locals: left.locals.join(&right.locals),
        stack: left.stack.join(&right.stack),
    })
}

/// Builds a handler-entry state only when no pre-initialization alias is live.
pub fn handler_entry_state(
    instruction: InstructionId,
    offset: usize,
    state: &VerificationState,
    exception: ReferenceType,
) -> Result<VerificationState, VerificationTransferError> {
    if frame_has_uninitialized(&state.locals) || frame_has_uninitialized(&state.stack) {
        return Err(VerificationTransferError {
            instruction,
            offset,
            opcode: Opcode::Athrow,
            kind: VerificationTransferKind::UninitializedHandlerEntry,
        });
    }
    let mut stack = VerificationFrame::new(FrameKind::OperandStack, state.stack.capacity());
    stack
        .push(VerificationType::Reference(exception))
        .map_err(|_| VerificationTransferError {
            instruction,
            offset,
            opcode: Opcode::Athrow,
            kind: VerificationTransferKind::StackBounds,
        })?;
    Ok(VerificationState {
        locals: state.locals.clone(),
        stack,
    })
}

fn descriptor_arguments(descriptor: &str) -> Option<Vec<VerificationType>> {
    let bytes = descriptor.as_bytes();
    if bytes.first() != Some(&b'(') {
        return None;
    }
    let mut cursor = 1;
    let mut arguments = Vec::new();
    while bytes.get(cursor) != Some(&b')') {
        arguments.push(parse_descriptor_type(descriptor, &mut cursor).ok()?);
    }
    Some(arguments)
}

fn replace_alias(frame: &mut VerificationFrame, from: &VerificationType, to: &VerificationType) {
    if let VerificationFrame::Reachable { slots, .. } = frame {
        for slot in slots.iter_mut() {
            if matches!(slot, Slot::Value(value) if value == from) {
                *slot = Slot::Value(to.clone());
            }
        }
    }
}

fn frame_has_uninitialized(frame: &VerificationFrame) -> bool {
    frame.normalized_slots().is_some_and(|slots| {
        slots.iter().any(|slot| {
            matches!(
                slot,
                Slot::Value(
                    VerificationType::Uninitialized(_) | VerificationType::UninitializedThis
                )
            )
        })
    })
}

fn reject_initialization_conflict(left: &VerificationFrame, right: &VerificationFrame) -> bool {
    match (left.normalized_slots(), right.normalized_slots()) {
        (Some(left), Some(right)) => {
            left.iter()
                .zip(right)
                .all(|(left, right)| match (left, right) {
                    (Slot::Value(a), Slot::Value(b)) => {
                        let a_uninit = matches!(
                            a,
                            VerificationType::Uninitialized(_)
                                | VerificationType::UninitializedThis
                        );
                        let b_uninit = matches!(
                            b,
                            VerificationType::Uninitialized(_)
                                | VerificationType::UninitializedThis
                        );
                        a_uninit == b_uninit && (!a_uninit || a == b)
                    }
                    _ => true,
                })
        }
        _ => true,
    }
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
    if frame_has_uninitialized(&state.locals) || frame_has_uninitialized(&state.stack) {
        return Err(fail(VerificationTransferKind::UninitializedUse));
    }
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
    Missing {
        /// Target instruction lacking its required declaration.
        instruction: InstructionId,
    },
    /// A declaration has a different shape or is not a supertype of the inferred state.
    NotAssignable {
        /// Target instruction carrying the incompatible declaration.
        instruction: InstructionId,
    },
    /// An inferred target state was unavailable after dataflow completed.
    MissingInference {
        /// Target instruction absent from the completed solution.
        instruction: InstructionId,
    },
}

/// Diagnostic policy for exception-table rows that no reachable throwing instruction enters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnreachableHandlerPolicy {
    /// Preserve unreachable rows as method-proof diagnostics.
    Report,
    /// Refuse a method containing an unreachable exception-table row.
    Refuse,
}

/// A sealed proof that every reachable instruction and exceptional path in one method was checked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MethodVerificationProof {
    fixpoint: ValueFingerprint,
    dependencies: Vec<ClassDefinitionId>,
    dependency_observations: Vec<Observation<ClassDefinitionId>>,
    unreachable_handlers: Box<[usize]>,
}

impl MethodVerificationProof {
    /// Content identity of the stable shared-dataflow fixpoint.
    pub const fn fixpoint(&self) -> ValueFingerprint {
        self.fixpoint
    }

    /// Loaded class definitions captured while validating catch assignability.
    pub fn dependencies(&self) -> &[ClassDefinitionId] {
        &self.dependencies
    }

    /// Exact content and class-space observations made while sealing the method.
    pub fn dependency_observations(&self) -> &[Observation<ClassDefinitionId>] {
        &self.dependency_observations
    }

    /// Exception-table rows that no reachable throwing instruction can enter.
    pub fn unreachable_handlers(&self) -> &[usize] {
        &self.unreachable_handlers
    }
}

/// Reason a whole-method proof could not be sealed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MethodVerificationError {
    /// The completion proof does not describe the supplied graph, policy, limits, or seeds.
    IncompleteFixpoint(CompletionProofMismatch),
    /// A catch type could not be resolved from its constant-pool identity.
    UnresolvedCatchType {
        /// Exception-table row.
        row: usize,
        /// Constant-pool class index.
        catch_type: u16,
    },
    /// A declared catch class is not assignable to `java/lang/Throwable`.
    CatchTypeNotThrowable {
        /// Exception-table row.
        row: usize,
        /// Constant-pool class index.
        catch_type: u16,
    },
    /// A bounded hierarchy query needed for catch validation failed.
    CatchTypeQuery {
        /// Exception-table row.
        row: usize,
        /// Bounded class-space refusal.
        error: VerificationQueryError,
    },
    /// The precise single-operand handler frame is absent from the converged target state.
    ExceptionalFrame {
        /// Exception-table row.
        row: usize,
        /// Handler target instruction.
        instruction: InstructionId,
    },
    /// A declared branch or handler target constraint was not proved.
    TargetConstraint(StackMapConstraintError),
    /// Policy requires every exception-table row to be reachable.
    UnreachableHandler {
        /// Exception-table row.
        row: usize,
    },
}

/// Seals a whole-method proof from the shared engine's content-bound completion signal.
#[allow(clippy::too_many_arguments)]
pub fn seal_method_verification<P, F>(
    proof: &DataflowCompletionProof<
        u32,
        VerificationEdgeId,
        VerificationEdgeClass,
        VerificationState,
    >,
    graph: &VerificationGraph,
    transfer: &AdmittedTransfer<P>,
    bottom: &VerificationState,
    seeds: impl IntoIterator<Item = (u32, VerificationState)>,
    budgets: QueryBudgets,
    environment: &VerificationEnvironment<'_>,
    mut catch_class: F,
    lineage_limit: usize,
    classfile_version: u16,
    declarations: &[ExpandedStackMapFrame],
    max_locals: usize,
    max_stack: usize,
    unreachable_policy: UnreachableHandlerPolicy,
) -> Result<MethodVerificationProof, MethodVerificationError>
where
    P: TransferPolicy<VerificationState>,
    F: FnMut(u16) -> Option<ReferenceType>,
{
    let solution = FixpointEngine::present(proof, graph, transfer, bottom, seeds, budgets)
        .map_err(MethodVerificationError::IncompleteFixpoint)?;
    let inferred = solution
        .states()
        .map(|(id, state)| (InstructionId(*id), state.clone()))
        .collect();
    check_stack_map_constraints(
        classfile_version,
        graph,
        &inferred,
        declarations,
        max_locals,
        max_stack,
    )
    .map_err(MethodVerificationError::TargetConstraint)?;

    let mut reached = std::collections::BTreeSet::new();
    let mut declared = std::collections::BTreeSet::new();
    for edge in graph.edges() {
        let EdgeClass::Custom(VerificationEdgeClass::Exceptional { row, catch_type }) =
            edge.class()
        else {
            continue;
        };
        declared.insert(*row);
        let exception = if *catch_type == 0 {
            ReferenceType::Class("java/lang/Throwable".into())
        } else {
            let caught =
                catch_class(*catch_type).ok_or(MethodVerificationError::UnresolvedCatchType {
                    row: *row,
                    catch_type: *catch_type,
                })?;
            match environment.reference_assignability(
                &caught,
                &ReferenceType::Class("java/lang/Throwable".into()),
                lineage_limit,
            ) {
                Ok(query) if query.value == VerificationAssignability::Assignable => {}
                Ok(_) => {
                    return Err(MethodVerificationError::CatchTypeNotThrowable {
                        row: *row,
                        catch_type: *catch_type,
                    });
                }
                Err(failure) => {
                    return Err(MethodVerificationError::CatchTypeQuery {
                        row: *row,
                        error: failure.error,
                    });
                }
            }
            caught
        };
        let Some(source) = solution.state(edge.source()) else {
            continue;
        };
        if source.locals.normalized_slots().is_none() {
            continue;
        }
        reached.insert(*row);
        let expected = handler_entry_state(
            InstructionId(*edge.source()),
            graph
                .node(edge.source())
                .expect("edge source exists")
                .location()
                .offset,
            source,
            exception,
        )
        .map_err(|_| MethodVerificationError::ExceptionalFrame {
            row: *row,
            instruction: InstructionId(*edge.target()),
        })?;
        let actual =
            solution
                .state(edge.target())
                .ok_or(MethodVerificationError::ExceptionalFrame {
                    row: *row,
                    instruction: InstructionId(*edge.target()),
                })?;
        if !expected.less_equal(actual) {
            return Err(MethodVerificationError::ExceptionalFrame {
                row: *row,
                instruction: InstructionId(*edge.target()),
            });
        }
    }
    let unreachable = declared.difference(&reached).copied().collect::<Vec<_>>();
    if unreachable_policy == UnreachableHandlerPolicy::Refuse {
        if let Some(row) = unreachable.first() {
            return Err(MethodVerificationError::UnreachableHandler { row: *row });
        }
    }
    let dependency_observations = environment
        .dependencies()
        .iter()
        .map(|dependency| {
            Observation::read(
                dependency.class().clone(),
                Revision::new(dependency.revision().number()),
                dependency.class().incremental_fingerprint(),
            )
        })
        .collect();
    Ok(MethodVerificationProof {
        fixpoint: proof.identity(),
        dependencies: environment
            .dependencies()
            .iter()
            .map(|dependency| dependency.class().clone())
            .collect(),
        dependency_observations,
        unreachable_handlers: unreachable.into_boxed_slice(),
    })
}

/// One method's stable identity inside a whole-class verification proof.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ClassMethodProofIdentity {
    method: String,
    proof: ValueFingerprint,
}

impl ClassMethodProofIdentity {
    /// Binds a declared method identity to its completed dataflow proof.
    pub fn new(method: impl Into<String>, proof: ValueFingerprint) -> Self {
        Self {
            method: method.into(),
            proof,
        }
    }

    /// Stable declared method identity (normally name plus descriptor).
    pub fn method(&self) -> &str {
        &self.method
    }

    /// Stable whole-method proof identity.
    pub const fn proof(&self) -> ValueFingerprint {
        self.proof
    }
}

/// Immutable proof for every structural constraint and method of one exact class.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClassVerificationProof {
    owner: ClassDefinitionId,
    owner_revision: ClassSpaceRevision,
    policy: ValueFingerprint,
    structural: ValueFingerprint,
    methods: Box<[ClassMethodProofIdentity]>,
    dependencies: Box<[Observation<ClassDefinitionId>]>,
    identity: ValueFingerprint,
}

impl ClassVerificationProof {
    #[cfg(test)]
    pub(crate) fn test(
        owner: ClassDefinitionId,
        owner_revision: ClassSpaceRevision,
        policy: ValueFingerprint,
        structural: ValueFingerprint,
        methods: &[&str],
    ) -> Self {
        let methods = methods
            .iter()
            .enumerate()
            .map(|(index, method)| {
                ClassMethodProofIdentity::new(*method, ValueFingerprint::new(index as u64 + 1))
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            owner,
            owner_revision,
            policy,
            structural,
            methods,
            dependencies: Box::new([]),
            identity: ValueFingerprint::new(99),
        }
    }

    /// Exact class definition proved.
    pub fn owner(&self) -> &ClassDefinitionId {
        &self.owner
    }

    /// Exact class-space revision observed while sealing this proof.
    pub const fn owner_revision(&self) -> ClassSpaceRevision {
        self.owner_revision
    }

    /// Exact verifier policy and schema used to produce this proof.
    pub const fn policy_fingerprint(&self) -> ValueFingerprint {
        self.policy
    }

    /// Fingerprint of class-level constraints (header, members, and attributes).
    pub const fn structural_fingerprint(&self) -> ValueFingerprint {
        self.structural
    }

    /// Method proofs in stable declared-method order.
    pub fn methods(&self) -> &[ClassMethodProofIdentity] {
        &self.methods
    }

    /// Deduplicated exact dependency observations, ordered by class identity.
    pub fn dependencies(&self) -> &[Observation<ClassDefinitionId>] {
        &self.dependencies
    }

    /// Content identity equal for incremental and clean recomputation.
    pub const fn identity(&self) -> ValueFingerprint {
        self.identity
    }
}

/// Refusal to aggregate incomplete or ambiguous method evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClassVerificationError {
    /// Two proofs claimed the same declared method identity.
    DuplicateMethod(String),
}

/// Aggregates stable method proofs and structural evidence into one exact class proof.
pub fn seal_class_verification(
    owner: &ClassDefinitionId,
    owner_revision: ClassSpaceRevision,
    policy: ValueFingerprint,
    structural: ValueFingerprint,
    methods: impl IntoIterator<Item = (String, MethodVerificationProof)>,
) -> Result<ClassVerificationProof, ClassVerificationError> {
    let mut identities = Vec::new();
    let mut dependencies = BTreeMap::new();
    dependencies.insert(
        owner.clone(),
        Observation::read(
            owner.clone(),
            Revision::new(owner_revision.number()),
            owner.incremental_fingerprint(),
        ),
    );
    for (method, proof) in methods {
        if identities
            .iter()
            .any(|identity: &ClassMethodProofIdentity| identity.method == method)
        {
            return Err(ClassVerificationError::DuplicateMethod(method));
        }
        identities.push(ClassMethodProofIdentity::new(method, proof.fixpoint));
        for observation in proof.dependency_observations {
            dependencies.insert(observation.key().clone(), observation);
        }
    }
    identities.sort();
    let dependencies = dependencies.into_values().collect::<Vec<_>>();
    let identity = (
        owner,
        policy,
        structural,
        &identities,
        dependencies
            .iter()
            .map(|observation| (observation.key(), observation.fingerprint()))
            .collect::<Vec<_>>(),
    )
        .incremental_fingerprint();
    Ok(ClassVerificationProof {
        owner: owner.clone(),
        owner_revision,
        policy,
        structural,
        methods: identities.into_boxed_slice(),
        dependencies: dependencies.into_boxed_slice(),
        identity,
    })
}

struct ClassProofCacheEntry {
    owner: Weak<ClassDefinition>,
    request: ValueFingerprint,
    proof: Arc<ClassVerificationProof>,
    _managed_proof: sim_lib_mutation::ManagedHandle,
}

/// Whole-class proof memo whose managed entries are ephemerons keyed by class mirrors.
#[derive(Default)]
pub struct ClassVerificationCache {
    entries: Mutex<BTreeMap<ClassDefinitionId, ClassProofCacheEntry>>,
}

impl ClassVerificationCache {
    /// Creates an empty whole-class proof memo.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reuses a proof only when its requested input and every observation remain exact.
    pub fn lookup<F>(
        &self,
        owner: &Arc<ClassDefinition>,
        request: ValueFingerprint,
        mut current: F,
    ) -> Option<Arc<ClassVerificationProof>>
    where
        F: FnMut(&ClassDefinitionId) -> Option<ValueFingerprint>,
    {
        let mut entries = self.entries();
        entries.retain(|_, entry| entry.owner.strong_count() != 0);
        let entry = entries.get(owner.id())?;
        if entry.request != request
            || entry
                .proof
                .dependencies
                .iter()
                .any(|observation| current(observation.key()) != observation.fingerprint())
        {
            return None;
        }
        Some(Arc::clone(&entry.proof))
    }

    /// Installs a proof under the managed class key without retaining that class.
    pub fn insert(
        &self,
        heap: &mut JvmHeap,
        cache: sim_lib_mutation::ManagedHandle,
        owner_handle: sim_lib_mutation::ManagedHandle,
        owner: &Arc<ClassDefinition>,
        request: ValueFingerprint,
        proof: ClassVerificationProof,
    ) -> Result<Arc<ClassVerificationProof>, JvmGraphError> {
        let managed_proof = heap.allocate(JvmRole::Cache).map_err(JvmGraphError::from)?;
        heap.ephemeron(cache, JvmEdge::DerivedEntry, owner_handle, managed_proof)?;
        let proof = Arc::new(proof);
        self.entries().insert(
            owner.id().clone(),
            ClassProofCacheEntry {
                owner: Arc::downgrade(owner),
                request,
                proof: Arc::clone(&proof),
                _managed_proof: managed_proof,
            },
        );
        Ok(proof)
    }

    /// Number of entries whose managed-class keys still exist.
    pub fn live_len(&self) -> usize {
        let mut entries = self.entries();
        entries.retain(|_, entry| entry.owner.strong_count() != 0);
        entries.len()
    }

    fn entries(&self) -> MutexGuard<'_, BTreeMap<ClassDefinitionId, ClassProofCacheEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
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
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
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
    use sim_lib_gc_tracing::CollectionLimits;
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

    fn observed_method(
        fixpoint: u64,
        dependencies: &[(&Arc<ClassDefinition>, ClassSpaceRevision)],
    ) -> MethodVerificationProof {
        MethodVerificationProof {
            fixpoint: ValueFingerprint::new(fixpoint),
            dependencies: dependencies
                .iter()
                .map(|(class, _)| class.id().clone())
                .collect(),
            dependency_observations: dependencies
                .iter()
                .map(|(class, revision)| {
                    Observation::read(
                        class.id().clone(),
                        Revision::new(revision.number()),
                        class.id().incremental_fingerprint(),
                    )
                })
                .collect(),
            unreachable_handlers: Box::new([]),
        }
    }

    fn collection_limits() -> CollectionLimits {
        CollectionLimits {
            objects: 32,
            edges: 32,
            stack: 32,
            work: 128,
            clears: 32,
            finalizers: 0,
        }
    }

    #[test]
    fn class_proofs_are_exact_incremental_and_collectible() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let loader = ClassLoader::new(4096);
        insert(&cx, &loader, "Base", &[], &[]);
        insert(
            &cx,
            &loader,
            "Owner",
            &["Base"],
            &[("a", "()V", 0), ("b", "()V", 0)],
        );
        insert(&cx, &loader, "Other", &[], &[("stable", "()V", 0)]);
        let base = loader.loaded("Base").unwrap().unwrap();
        let owner = loader.loaded("Owner").unwrap().unwrap();
        let other = loader.loaded("Other").unwrap().unwrap();
        let revision = loader.revision();
        let methods = || {
            vec![
                (
                    "a()V".into(),
                    observed_method(11, &[(&owner, revision), (&base, revision)]),
                ),
                ("b()V".into(), observed_method(12, &[(&owner, revision)])),
            ]
        };
        let structural = ValueFingerprint::new(20);
        let clean = seal_class_verification(
            owner.id(),
            revision,
            ValueFingerprint::new(7),
            structural,
            methods(),
        )
        .unwrap();
        let incremental = seal_class_verification(
            owner.id(),
            revision,
            ValueFingerprint::new(7),
            structural,
            methods(),
        )
        .unwrap();
        assert_eq!(incremental.identity(), clean.identity());
        assert_eq!(clean.dependencies().len(), 2);

        let mut heap = JvmHeap::new(8, collection_limits()).unwrap();
        let managed_cache = heap.allocate(JvmRole::Cache).unwrap();
        let managed_owner = heap.allocate(JvmRole::ClassMirror).unwrap();
        let managed_other = heap.allocate(JvmRole::ClassMirror).unwrap();
        let cache_root = heap.root(managed_cache).unwrap();
        let owner_root = heap.root(managed_owner).unwrap();
        let other_root = heap.root(managed_other).unwrap();
        let cache = ClassVerificationCache::new();
        let request = (owner.id(), structural, clean.methods()).incremental_fingerprint();
        let cached = cache
            .insert(
                &mut heap,
                managed_cache,
                managed_owner,
                &owner,
                request,
                clean,
            )
            .unwrap();
        let other_structural = ValueFingerprint::new(30);
        let other_proof = seal_class_verification(
            other.id(),
            revision,
            ValueFingerprint::new(7),
            other_structural,
            [(
                "stable()V".into(),
                observed_method(31, &[(&other, revision)]),
            )],
        )
        .unwrap();
        let other_request =
            (other.id(), other_structural, other_proof.methods()).incremental_fingerprint();
        cache
            .insert(
                &mut heap,
                managed_cache,
                managed_other,
                &other,
                other_request,
                other_proof,
            )
            .unwrap();
        let current = |id: &ClassDefinitionId| {
            loader
                .loaded(id.binary_name())
                .ok()
                .flatten()
                .filter(|class| class.id() == id)
                .map(|class| class.id().incremental_fingerprint())
        };
        assert!(Arc::ptr_eq(
            &cached,
            &cache.lookup(&owner, request, current).unwrap()
        ));
        let edited_method_request = ValueFingerprint::new(request.get().wrapping_add(1));
        assert!(
            cache
                .lookup(&owner, edited_method_request, current)
                .is_none()
        );

        let replacement = JavaClassMetadata::test_class(&cx, "Base", &[], 0, &[("new", "()V", 0)]);
        loader.test_insert(ClassDefinition::test(
            loader.id(),
            "Base",
            999,
            replacement,
            BTreeMap::new(),
        ));
        assert!(cache.lookup(&owner, request, current).is_none());
        assert!(cache.lookup(&other, other_request, current).is_some());

        heap.release_root(owner_root).unwrap();
        let receipt = heap.collect().unwrap();
        assert_eq!(receipt.cleared_ephemerons.len(), 1);
        assert_eq!(receipt.cleared_ephemerons[0].0, managed_cache.id());
        assert!(receipt.swept.contains(&managed_owner.id()));
        assert!(!receipt.swept.contains(&managed_other.id()));
        heap.release_root(other_root).unwrap();
        heap.release_root(cache_root).unwrap();
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
    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, SourceId};

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
                Opcode::New => (NONE, INT),
                Opcode::Invokespecial => (INT, NONE),
                Opcode::Return | Opcode::Goto | Opcode::Jsr | Opcode::Ret => (NONE, NONE),
                _ if verifier_rule(opcode).family == VerifierRuleFamily::ConstantsLocalsStack => {
                    (NONE, NONE)
                }
                _ if verifier_rule(opcode).family == VerifierRuleFamily::NumericConversion => {
                    (NONE, NONE)
                }
                _ if verifier_rule(opcode).family == VerifierRuleFamily::ObjectArrayField => {
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

    fn field_pool() -> ConstantPool {
        let bytes = [
            0, 7, 1, 0, 5, b'O', b'w', b'n', b'e', b'r', 7, 0, 1, 1, 0, 5, b'v', b'a', b'l', b'u',
            b'e', 1, 0, 1, b'I', 12, 0, 3, 0, 4, 9, 0, 2, 0, 5,
        ];
        ConstantPool::decode(&mut ByteReader::new(&bytes, bytes.len()), 61).unwrap()
    }

    fn invocation_pool(tag: u8) -> ConstantPool {
        let mut bytes = vec![0, if tag == 18 { 6 } else { 7 }, tag];
        if tag == 18 {
            bytes.extend_from_slice(&[0, 0, 0, 2]);
            bytes.extend_from_slice(&[12, 0, 3, 0, 4]);
            bytes.extend_from_slice(&[
                1, 0, 4, b'w', b'o', b'r', b'k', 1, 0, 4, b'(', b'I', b')', b'J', 1, 0, 1, b'X',
            ]);
        } else {
            bytes.extend_from_slice(&[0, 2, 0, 3, 7, 0, 6, 12, 0, 4, 0, 5]);
            bytes.extend_from_slice(&[
                1, 0, 4, b'w', b'o', b'r', b'k', 1, 0, 4, b'(', b'I', b')', b'J', 1, 0, 12, b's',
                b'a', b'm', b'p', b'l', b'e', b'/', b'O', b'w', b'n', b'e', b'r',
            ]);
        }
        ConstantPool::decode(&mut ByteReader::new(&bytes, bytes.len()), 61).unwrap()
    }

    fn test_method(descriptor: &str, access_flags: u16) -> JavaMember {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        JavaClassMetadata::test_class(&cx, "Owner", &[], 0, &[("work", descriptor, access_flags)])
            .select_method("work", descriptor)
            .unwrap()
            .clone()
    }

    fn prepared(bytes: &[u8], handlers: &[CodeException]) -> LocatedCode<PreparedJvmPolicy> {
        prepared_with_pool(bytes, handlers, &empty_pool())
    }

    fn prepared_with_pool(
        bytes: &[u8],
        handlers: &[CodeException],
        pool: &ConstantPool,
    ) -> LocatedCode<PreparedJvmPolicy> {
        let decoded = decode_instructions(bytes, 61, pool).unwrap();
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
    fn allocation_sites_of_the_same_class_remain_distinct_types() {
        let pool = ConstantPool::decode(
            &mut ByteReader::new(
                &[
                    0, 3, 7, 0, 2, 1, 0, 12, b's', b'a', b'm', b'p', b'l', b'e', b'/', b'V', b'a',
                    b'l', b'u', b'e',
                ],
                64,
            ),
            61,
        )
        .unwrap();
        let code = prepared_with_pool(
            &[Opcode::New as u8, 0, 1, Opcode::New as u8, 0, 1],
            &[],
            &pool,
        );
        let initial = VerificationState {
            locals: VerificationFrame::new(FrameKind::Locals, 0),
            stack: VerificationFrame::new(FrameKind::OperandStack, 2),
        };
        let first = transfer_new_instruction(
            code.instruction(code.cursor(InstructionId(0)).unwrap())
                .instruction(),
            0,
            &initial,
        )
        .unwrap();
        let second = transfer_new_instruction(
            code.instruction(code.cursor(InstructionId(1)).unwrap())
                .instruction(),
            3,
            &first,
        )
        .unwrap();
        assert_eq!(
            stack_values(&second.stack),
            vec![
                VerificationType::Uninitialized(0),
                VerificationType::Uninitialized(1)
            ]
        );
    }

    #[test]
    fn successful_constructor_replaces_every_alias() {
        let pool_bytes = [
            0, 7, 10, 0, 2, 0, 3, 7, 0, 4, 12, 0, 5, 0, 6, 1, 0, 12, b's', b'a', b'm', b'p', b'l',
            b'e', b'/', b'V', b'a', b'l', b'u', b'e', 1, 0, 6, b'<', b'i', b'n', b'i', b't', b'>',
            1, 0, 3, b'(', b')', b'V',
        ];
        let pool =
            ConstantPool::decode(&mut ByteReader::new(&pool_bytes, pool_bytes.len()), 61).unwrap();
        let code = prepared_with_pool(&[Opcode::Invokespecial as u8, 0, 1], &[], &pool);
        let alias = VerificationType::Uninitialized(7);
        let mut locals = VerificationFrame::new(FrameKind::Locals, 2);
        locals.set_local(0, alias.clone()).unwrap();
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 2);
        stack.push(alias.clone()).unwrap();
        stack.push(alias).unwrap();
        let next = transfer_constructor_instruction(
            code.instruction(code.cursor(InstructionId(0)).unwrap())
                .instruction(),
            0,
            &VerificationState { locals, stack },
            &VerificationConstructor {
                owner: "sample/Value".into(),
                name: "<init>".into(),
                descriptor: "()V".into(),
                receiver: VerificationType::Uninitialized(7),
            },
        )
        .unwrap();
        let initialized = VerificationType::Reference(ReferenceType::Class("sample/Value".into()));
        assert_eq!(next.locals.get(0), Some(&initialized));
        assert_eq!(stack_values(&next.stack), vec![initialized]);
    }

    #[test]
    fn every_invocation_kind_checks_descriptor_receiver_and_owner_kind() {
        let pool = invocation_pool(10);
        let loader = crate::ClassLoader::new(1);
        let environment = VerificationEnvironment::new(&loader, 1);
        let cases = [
            (Opcode::Invokevirtual, false, 0),
            (Opcode::Invokespecial, false, 0),
            (Opcode::Invokestatic, false, 0x0008),
        ];
        for (opcode, owner_is_interface, flags) in cases {
            let suffix: &[u8] = if opcode == Opcode::Invokeinterface {
                &[2, 0]
            } else {
                &[]
            };
            let bytes = [opcode as u8, 0, 1]
                .into_iter()
                .chain(suffix.iter().copied())
                .collect::<Vec<_>>();
            let code = prepared_with_pool(&bytes, &[], &pool);
            let method = test_method("(I)J", flags);
            let mut stack = VerificationFrame::new(FrameKind::OperandStack, 4);
            if opcode != Opcode::Invokestatic {
                stack
                    .push(VerificationType::Reference(ReferenceType::Class(
                        "Owner".into(),
                    )))
                    .unwrap();
            }
            stack.push(VerificationType::Int).unwrap();
            let next = transfer_invocation_instruction(
                code.instruction(code.cursor(InstructionId(0)).unwrap())
                    .instruction(),
                0,
                &VerificationState {
                    locals: VerificationFrame::new(FrameKind::Locals, 0),
                    stack,
                },
                &VerificationInvocation {
                    owner: "Owner",
                    owner_is_interface,
                    method: &method,
                    accessible: true,
                    signature_polymorphic: false,
                },
                &environment,
                1,
            )
            .unwrap();
            assert_eq!(stack_values(&next.stack), [VerificationType::Long]);
        }

        let pool = invocation_pool(11);
        let code = prepared_with_pool(&[Opcode::Invokeinterface as u8, 0, 1, 2, 0], &[], &pool);
        let method = test_method("(I)J", 0);
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 3);
        stack.push(VerificationType::Null).unwrap();
        stack.push(VerificationType::Int).unwrap();
        transfer_invocation_instruction(
            code.instruction(code.cursor(InstructionId(0)).unwrap())
                .instruction(),
            0,
            &VerificationState {
                locals: VerificationFrame::new(FrameKind::Locals, 0),
                stack,
            },
            &VerificationInvocation {
                owner: "Owner",
                owner_is_interface: true,
                method: &method,
                accessible: true,
                signature_polymorphic: false,
            },
            &environment,
            1,
        )
        .unwrap();
    }

    #[test]
    fn dynamic_verification_reuses_executor_identity_without_linkage() {
        let pool = invocation_pool(18);
        let code = prepared_with_pool(&[Opcode::Invokedynamic as u8, 0, 1, 0, 0], &[], &pool);
        let instruction = code
            .instruction(code.cursor(InstructionId(0)).unwrap())
            .instruction();
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 3);
        stack.push(VerificationType::Int).unwrap();
        let state = VerificationState {
            locals: VerificationFrame::new(FrameKind::Locals, 0),
            stack,
        };
        let refused = DynamicBootstrap {
            owner: "sample/Bootstrap".into(),
            name: "link".into(),
            descriptor: "()V".into(),
        };
        let error = transfer_dynamic_invocation_instruction(
            instruction,
            0,
            &state,
            &VerificationDynamicInvocation {
                bootstrap: &refused,
                descriptor: "(I)J",
            },
        )
        .unwrap_err();
        assert_eq!(
            error.kind,
            VerificationTransferKind::DynamicBootstrap(DynamicLinkError::UnadmittedBootstrap {
                owner: refused.owner.clone(),
                name: refused.name.clone(),
                descriptor: refused.descriptor.clone(),
            })
        );

        let cache = crate::DynamicLinkCache::new();
        let admitted = DynamicBootstrap {
            owner: STRING_CONCAT_BOOTSTRAP_OWNER.into(),
            name: STRING_CONCAT_BOOTSTRAP_NAME.into(),
            descriptor: STRING_CONCAT_BOOTSTRAP_DESCRIPTOR.into(),
        };
        let next = transfer_dynamic_invocation_instruction(
            instruction,
            0,
            &state,
            &VerificationDynamicInvocation {
                bootstrap: &admitted,
                descriptor: "(I)J",
            },
        )
        .unwrap();
        assert_eq!(stack_values(&next.stack), [VerificationType::Long]);
        assert_eq!(
            cache.live_len(),
            0,
            "verification must not link or allocate a cache entry"
        );
    }

    #[test]
    fn initialized_uninitialized_backward_merge_is_refused() {
        let mut left = VerificationFrame::new(FrameKind::Locals, 1);
        left.set_local(0, VerificationType::Uninitialized(2))
            .unwrap();
        let mut right = VerificationFrame::new(FrameKind::Locals, 1);
        right
            .set_local(
                0,
                VerificationType::Reference(ReferenceType::Class("sample/Value".into())),
            )
            .unwrap();
        let error = join_initialization_states(
            InstructionId(1),
            4,
            &VerificationState {
                locals: left,
                stack: VerificationFrame::new(FrameKind::OperandStack, 0),
            },
            &VerificationState {
                locals: right,
                stack: VerificationFrame::new(FrameKind::OperandStack, 0),
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, VerificationTransferKind::InitializationMerge);
    }

    #[test]
    fn handler_entry_refuses_a_live_uninitialized_alias() {
        let mut locals = VerificationFrame::new(FrameKind::Locals, 1);
        locals
            .set_local(0, VerificationType::Uninitialized(4))
            .unwrap();
        let error = handler_entry_state(
            InstructionId(3),
            8,
            &VerificationState {
                locals,
                stack: VerificationFrame::new(FrameKind::OperandStack, 1),
            },
            ReferenceType::Class("java/lang/Throwable".into()),
        )
        .unwrap_err();
        assert_eq!(
            error.kind,
            VerificationTransferKind::UninitializedHandlerEntry
        );
    }

    #[test]
    fn handler_entry_has_exact_single_catch_operand_and_preserves_locals() {
        let mut locals = VerificationFrame::new(FrameKind::Locals, 1);
        locals.set_local(0, VerificationType::Int).unwrap();
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 3);
        stack.push(VerificationType::Long).unwrap();
        let entered = handler_entry_state(
            InstructionId(4),
            9,
            &VerificationState {
                locals: locals.clone(),
                stack,
            },
            ReferenceType::Class("sample/Caught".into()),
        )
        .unwrap();
        assert_eq!(entered.locals, locals);
        assert_eq!(
            stack_values(&entered.stack),
            [VerificationType::Reference(ReferenceType::Class(
                "sample/Caught".into()
            ))]
        );
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

    fn memory_transfer(
        opcode: Opcode,
        input: Vec<VerificationType>,
        field: Option<&VerificationField<'_>>,
    ) -> Result<VerificationState, VerificationTransferError> {
        let bytes = if matches!(
            opcode,
            Opcode::Getstatic
                | Opcode::Putstatic
                | Opcode::Getfield
                | Opcode::Putfield
                | Opcode::Checkcast
                | Opcode::Instanceof
                | Opcode::Anewarray
        ) {
            vec![opcode as u8, 0, 6]
        } else {
            vec![opcode as u8]
        };
        let pool = if matches!(
            opcode,
            Opcode::Getstatic | Opcode::Putstatic | Opcode::Getfield | Opcode::Putfield
        ) {
            field_pool()
        } else {
            empty_pool()
        };
        let decoded = decode_instructions(&bytes, 61, &pool).unwrap();
        let code = prepare_code::<GraphPolicy>(
            &decoded,
            bytes.len(),
            &[],
            SourceId("Verifier.memory()V".into()),
        )
        .unwrap();
        let instruction = code.instruction(code.entry()).instruction();
        let state = VerificationState {
            locals: VerificationFrame::new(FrameKind::Locals, 0),
            stack: stack_from_values(8, input).unwrap(),
        };
        transfer_memory_instruction(instruction, 0, &state, field)
    }

    #[test]
    fn aaload_refuses_a_primitive_array_under_jvms_4_10_1_9() {
        let error = memory_transfer(
            Opcode::Aaload,
            vec![
                VerificationType::Reference(ReferenceType::Array("[I".into())),
                VerificationType::Int,
            ],
            None,
        )
        .unwrap_err();
        assert_eq!(error.kind, VerificationTransferKind::ArrayType);
    }

    #[test]
    fn protected_field_receiver_obeys_jvms_4_10_1_8() {
        let declaration = JavaMember::test_field("value", "I", 0x0004);
        let field = VerificationField {
            declaring: "base.Owner",
            field: &declaration,
            accessible: true,
            caller_is_subclass: true,
            caller: "other.Child",
        };
        let error = memory_transfer(
            Opcode::Getfield,
            vec![VerificationType::Reference(ReferenceType::Class(
                "unrelated.Peer".into(),
            ))],
            Some(&field),
        )
        .unwrap_err();
        assert_eq!(error.kind, VerificationTransferKind::ProtectedMemberAccess);
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

#[cfg(test)]
#[path = "verifier_adversarial_tests.rs"]
mod adversarial_tests;
