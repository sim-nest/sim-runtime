use sim_kernel::Origin;
use sim_lib_control::Raised;

/// Every failure condition recognized at the JVM profile boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureCondition {
    /// Guest execution dereferenced null.
    NullDereference,
    /// Guest integer or long division used a zero divisor.
    Arithmetic,
    /// A guest reference failed a checked cast.
    ClassCast,
    /// A guest array access used an index outside its fixed bounds.
    ArrayIndexOutOfBounds,
    /// A guest attempted to store an incompatible reference in an array.
    ArrayStore,
    /// A guest requested an array with a negative length.
    NegativeArraySize,
    /// Decoded class data violates the supported classfile contract.
    InvalidClassfile,
    /// A class or member cannot be authorized in the supplied class space.
    UnauthorizedLinkage,
    /// A declared execution limit is invalid or code exceeds it before execution.
    ExecutionAdmissionLimit,
    /// The operand stack cannot accept another complete logical value.
    OperandCapacity,
    /// The bounded frame stack cannot accept another frame.
    FrameCapacity,
    /// The explicit execution-work allowance is exhausted.
    WorkBudget,
    /// The managed-object allowance is exhausted.
    ManagedObjectBudget,
    /// The classfile-byte allowance is exhausted.
    ClassfileByteBudget,
    /// The interned-string allowance is exhausted.
    InternedStringBudget,
}

/// The single boundary owning a failure condition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureHome {
    /// Guest-visible exceptional completion carried by `Raised`.
    JavaThrowable,
    /// Located refusal before admitted execution exists.
    Admission,
    /// Bounded resource exhaustion during or between execution steps.
    Resource,
}

impl FailureCondition {
    /// Classifies every condition into exactly one failure home.
    pub const fn home(self) -> FailureHome {
        match self {
            Self::NullDereference
            | Self::Arithmetic
            | Self::ClassCast
            | Self::ArrayIndexOutOfBounds
            | Self::ArrayStore
            | Self::NegativeArraySize => FailureHome::JavaThrowable,
            Self::InvalidClassfile | Self::UnauthorizedLinkage | Self::ExecutionAdmissionLimit => {
                FailureHome::Admission
            }
            Self::OperandCapacity
            | Self::FrameCapacity
            | Self::WorkBudget
            | Self::ManagedObjectBudget
            | Self::ClassfileByteBudget
            | Self::InternedStringBudget => FailureHome::Resource,
        }
    }

    /// Exact guest throwable class for throwable-owned conditions.
    pub const fn java_class(self) -> Option<&'static str> {
        match self {
            Self::NullDereference => Some("java/lang/NullPointerException"),
            Self::Arithmetic => Some("java/lang/ArithmeticException"),
            Self::ClassCast => Some("java/lang/ClassCastException"),
            Self::ArrayIndexOutOfBounds => Some("java/lang/ArrayIndexOutOfBoundsException"),
            Self::ArrayStore => Some("java/lang/ArrayStoreException"),
            Self::NegativeArraySize => Some("java/lang/NegativeArraySizeException"),
            _ => None,
        }
    }
}

/// Guest-visible Java exceptional completion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JavaThrowable {
    condition: FailureCondition,
    /// The shared language-neutral raised envelope.
    raised: Raised,
}

impl JavaThrowable {
    /// Constructs a guest throwable only for throwable-owned conditions.
    pub fn new(condition: FailureCondition, raised: Raised) -> Option<Self> {
        (condition.home() == FailureHome::JavaThrowable).then_some(Self { condition, raised })
    }

    /// Returns the guest throwable condition.
    pub const fn condition(&self) -> FailureCondition {
        self.condition
    }

    /// Returns the shared raised envelope.
    pub const fn raised(&self) -> &Raised {
        &self.raised
    }
}

/// A refusal discovered before an execution permit exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdmissionFailure {
    condition: FailureCondition,
    origin: Origin,
}

impl AdmissionFailure {
    /// Constructs a located admission refusal only for admission-owned conditions.
    pub fn new(condition: FailureCondition, origin: Origin) -> Option<Self> {
        (condition.home() == FailureHome::Admission).then_some(Self { condition, origin })
    }

    /// Returns the admission condition.
    pub const fn condition(&self) -> FailureCondition {
        self.condition
    }

    /// Returns exact input provenance at which admission failed.
    pub const fn origin(&self) -> &Origin {
        &self.origin
    }
}

/// Resource exhaustion evidence, distinct from guest throwables and admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceFailure {
    condition: FailureCondition,
    used: usize,
    limit: usize,
}

impl ResourceFailure {
    /// Constructs resource evidence only for resource-owned conditions.
    pub fn new(condition: FailureCondition, used: usize, limit: usize) -> Option<Self> {
        (condition.home() == FailureHome::Resource).then_some(Self {
            condition,
            used,
            limit,
        })
    }

    /// Returns the exhausted resource condition.
    pub const fn condition(self) -> FailureCondition {
        self.condition
    }

    /// Returns consumption observed at failure.
    pub const fn used(self) -> usize {
        self.used
    }

    /// Returns the configured hard bound.
    pub const fn limit(self) -> usize {
        self.limit
    }
}
