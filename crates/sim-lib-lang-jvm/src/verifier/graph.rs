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

include!("../verifier_rules_generated.rs");

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
