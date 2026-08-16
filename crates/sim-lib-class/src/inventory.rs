//! Machine-readable ownership, reuse, exclusion, and non-goal ledgers.

/// Candidate semantics examined before assigning concrete class ownership.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CandidateModel {
    KernelClassProtocol,
    PythonClassSpace,
    JavascriptPrototype,
    LuaMetatable,
    TypeclassDictionary,
    PrologRelation,
    GenericDispatch,
    JvmLoaderIdentity,
}

/// How this crate treats a candidate implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateDisposition {
    /// Use the contract without moving its ownership.
    Reuse,
    /// Reuse an algorithm, but not the candidate's language-specific object policy.
    Compose,
    /// The candidate is not class inheritance.
    Exclude,
}

/// The semantic domain actually governed by a candidate model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticDomain {
    ClassProtocol,
    ClassInheritance,
    PropertyLookup,
    MetamethodLookup,
    ConstraintEvidence,
    LogicalRelation,
    MethodSelection,
    RuntimeTypeIdentity,
}

/// Exact meaning that an edge or declaration has in a candidate model.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParentMeaning {
    /// A direct superclass contributes ancestry and participates in C3 order.
    DeclaredSuperclass,
    /// A protocol reports direct parents but owns no concrete hierarchy.
    ProtocolReportedSuperclass,
    /// An object delegates missing property reads to another object.
    PrototypeDelegate,
    /// A table delegates selected operations through `__index` and metamethods.
    MetatableDelegate,
    /// A dictionary witnesses that a type satisfies a named constraint.
    ConstraintWitness,
    /// A predicate relates terms; the word "parent" has no privileged meaning.
    PredicateArgument,
    /// Applicability and Shape specificity order methods for one invocation.
    ApplicableMethod,
    /// Defining-loader plus binary-name identifies a runtime type.
    DefiningLoaderIdentity,
}

/// One complete candidate row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticCandidate {
    pub model: CandidateModel,
    pub disposition: CandidateDisposition,
    pub domain: SemanticDomain,
    pub declared_parent: ParentMeaning,
    pub source_anchor: &'static str,
}

/// Structured reason an adjacent model cannot implement declared class parents.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExclusionReason {
    pub model: CandidateModel,
    /// Meaning required by this crate's declared `parents` field.
    pub required: ParentMeaning,
    /// Meaning supplied by the excluded model.
    pub actual: ParentMeaning,
    /// Stable code consumed by ownership guards; never infer this from prose.
    pub mismatch_code: &'static str,
}

/// Precise behavior intentionally outside this crate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NonGoal {
    pub model: CandidateModel,
    pub excluded_semantics: &'static str,
}

pub const fn candidate_inventory() -> &'static [SemanticCandidate] {
    &[
        SemanticCandidate {
            model: CandidateModel::KernelClassProtocol,
            disposition: CandidateDisposition::Reuse,
            domain: SemanticDomain::ClassProtocol,
            declared_parent: ParentMeaning::ProtocolReportedSuperclass,
            source_anchor: "sim-kernel/src/class.rs",
        },
        SemanticCandidate {
            model: CandidateModel::PythonClassSpace,
            disposition: CandidateDisposition::Compose,
            domain: SemanticDomain::ClassInheritance,
            declared_parent: ParentMeaning::DeclaredSuperclass,
            source_anchor: "sim-runtime/crates/sim-lib-lang-python/src/objects.rs",
        },
        SemanticCandidate {
            model: CandidateModel::JavascriptPrototype,
            disposition: CandidateDisposition::Exclude,
            domain: SemanticDomain::PropertyLookup,
            declared_parent: ParentMeaning::PrototypeDelegate,
            source_anchor: "sim-runtime/crates/sim-lib-lang-javascript/src/objects/space.rs",
        },
        SemanticCandidate {
            model: CandidateModel::LuaMetatable,
            disposition: CandidateDisposition::Exclude,
            domain: SemanticDomain::MetamethodLookup,
            declared_parent: ParentMeaning::MetatableDelegate,
            source_anchor: "sim-runtime/crates/sim-lib-lang-lua/src/metatable.rs",
        },
        SemanticCandidate {
            model: CandidateModel::TypeclassDictionary,
            disposition: CandidateDisposition::Exclude,
            domain: SemanticDomain::ConstraintEvidence,
            declared_parent: ParentMeaning::ConstraintWitness,
            source_anchor: "sim-runtime/crates/sim-lib-lang-typed-lazy/src/runtime.rs",
        },
        SemanticCandidate {
            model: CandidateModel::PrologRelation,
            disposition: CandidateDisposition::Exclude,
            domain: SemanticDomain::LogicalRelation,
            declared_parent: ParentMeaning::PredicateArgument,
            source_anchor: "sim-runtime/crates/sim-lib-lang-prolog/src/surface.rs",
        },
        SemanticCandidate {
            model: CandidateModel::GenericDispatch,
            disposition: CandidateDisposition::Exclude,
            domain: SemanticDomain::MethodSelection,
            declared_parent: ParentMeaning::ApplicableMethod,
            source_anchor: "sim-runtime/crates/sim-lib-dispatch/src/method.rs",
        },
        SemanticCandidate {
            model: CandidateModel::JvmLoaderIdentity,
            disposition: CandidateDisposition::Exclude,
            domain: SemanticDomain::RuntimeTypeIdentity,
            declared_parent: ParentMeaning::DefiningLoaderIdentity,
            source_anchor: "no-owner:index-and-source-inventory",
        },
    ]
}

pub const fn exclusion_ledger() -> &'static [ExclusionReason] {
    &[
        ExclusionReason {
            model: CandidateModel::JavascriptPrototype,
            required: ParentMeaning::DeclaredSuperclass,
            actual: ParentMeaning::PrototypeDelegate,
            mismatch_code: "prototype-delegates-properties-not-declared-ancestry",
        },
        ExclusionReason {
            model: CandidateModel::LuaMetatable,
            required: ParentMeaning::DeclaredSuperclass,
            actual: ParentMeaning::MetatableDelegate,
            mismatch_code: "metatable-delegates-operations-not-declared-ancestry",
        },
        ExclusionReason {
            model: CandidateModel::TypeclassDictionary,
            required: ParentMeaning::DeclaredSuperclass,
            actual: ParentMeaning::ConstraintWitness,
            mismatch_code: "dictionary-witnesses-constraint-not-subclass-edge",
        },
        ExclusionReason {
            model: CandidateModel::PrologRelation,
            required: ParentMeaning::DeclaredSuperclass,
            actual: ParentMeaning::PredicateArgument,
            mismatch_code: "predicate-name-has-no-inheritance-semantics",
        },
        ExclusionReason {
            model: CandidateModel::GenericDispatch,
            required: ParentMeaning::DeclaredSuperclass,
            actual: ParentMeaning::ApplicableMethod,
            mismatch_code: "specificity-orders-methods-not-classes",
        },
        ExclusionReason {
            model: CandidateModel::JvmLoaderIdentity,
            required: ParentMeaning::DeclaredSuperclass,
            actual: ParentMeaning::DefiningLoaderIdentity,
            mismatch_code: "loader-qualifies-type-identity-not-parent-resolution",
        },
    ]
}

pub const fn non_goals() -> &'static [NonGoal] {
    &[
        NonGoal {
            model: CandidateModel::JavascriptPrototype,
            excluded_semantics: "prototype mutation, property descriptors, private brands, and constructor/new policy",
        },
        NonGoal {
            model: CandidateModel::LuaMetatable,
            excluded_semantics: "__index traversal, metamethod invocation, and raw table access",
        },
        NonGoal {
            model: CandidateModel::TypeclassDictionary,
            excluded_semantics: "constraint inference, instance coherence, and dictionary method lookup",
        },
        NonGoal {
            model: CandidateModel::PrologRelation,
            excluded_semantics: "unification, backtracking, clauses, and relation evaluation",
        },
        NonGoal {
            model: CandidateModel::GenericDispatch,
            excluded_semantics: "method applicability, Shape specificity, method combination, and invocation",
        },
        NonGoal {
            model: CandidateModel::JvmLoaderIdentity,
            excluded_semantics: "classfile loading, verification, initialization, linking, and defining-loader namespaces",
        },
    ]
}
