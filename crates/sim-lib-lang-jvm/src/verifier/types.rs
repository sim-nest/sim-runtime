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
