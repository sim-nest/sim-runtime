/// JavaScript property keys. Private names are deliberately class-scoped and
/// cannot be manufactured from strings.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum JavascriptPropertyKey {
    /// String property.
    String(String),
    /// Symbol identity (ordered after strings).
    Symbol(u64),
    /// Declared private name, paired with its declaring class identity.
    Private {
        /// Declaring class brand.
        class: ManagedId,
        /// Source-level declared name.
        name: String,
    },
}
/// The ordinary function forms admitted by this profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JavascriptFunctionKind {
    /// Ordinary dynamically-received function.
    Function,
    /// Lexically-received arrow closure.
    Arrow,
    /// Class constructor, callable only through construction.
    ClassConstructor,
}

/// JavaScript policy retained beside a language-neutral function plan.
#[derive(Clone, Debug)]
pub struct JavascriptFunctionPolicy {
    /// Function form and its receiver policy.
    pub kind: JavascriptFunctionKind,
    /// Whether `new` is legal.
    pub constructable: bool,
    /// Declaration-time default values, keyed by frozen parameter name.
    pub defaults: BTreeMap<Symbol, JavascriptValue>,
    /// Whether invocation creates an async continuation.
    pub asynchronous: bool,
    /// Whether invocation creates a generator frame.
    pub generator: bool,
    /// Stable realm identity used by JavaScript intrinsic lookup.
    pub realm: Symbol,
    /// Stable source origin used for JavaScript errors.
    pub error_origin: String,
}

/// A JavaScript call-binding failure with its guest-owned source origin.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JavascriptCallError {
    /// Stable source origin.
    pub origin: String,
    /// Human-readable ECMAScript binding failure.
    pub message: String,
}

/// Inspectable callable metadata. Shared plans and capture cells own neutral
/// mechanics; this record retains only JavaScript and object-system policy.
#[derive(Clone, Debug)]
pub struct JavascriptFunction {
    plan: FunctionPlan,
    captures: Vec<CapturedBinding>,
    policy: JavascriptFunctionPolicy,
    /// Declared private names for a class constructor.
    pub private_names: Vec<String>,
}

impl JavascriptFunction {
    /// Freezes a JavaScript function over an already validated neutral plan.
    pub fn new(
        plan: FunctionPlan,
        captures: Vec<CapturedBinding>,
        policy: JavascriptFunctionPolicy,
        private_names: Vec<String>,
    ) -> Result<Self, InstanceError> {
        validate_capture_bindings(&plan, &captures)?;
        Ok(Self {
            plan,
            captures,
            policy,
            private_names,
        })
    }

    /// Borrows the shared immutable declaration plan.
    pub const fn plan(&self) -> &FunctionPlan {
        &self.plan
    }

    /// Borrows exact capture cells in frozen plan order.
    pub fn captures(&self) -> &[CapturedBinding] {
        &self.captures
    }

    /// Borrows JavaScript-only callable policy.
    pub const fn policy(&self) -> &JavascriptFunctionPolicy {
        &self.policy
    }

    /// Applies ECMAScript positional, default, and rest rules to raw values.
    pub fn bind_arguments(
        &self,
        arguments: &[JavascriptValue],
    ) -> Result<BTreeMap<Symbol, Vec<JavascriptValue>>, JavascriptCallError> {
        let mut bound = BTreeMap::new();
        let mut at = 0;
        for parameter in self.plan.parameters() {
            let values = match parameter.kind() {
                ParameterKind::Remainder => {
                    let values = arguments[at..].to_vec();
                    at = arguments.len();
                    values
                }
                ParameterKind::Required => {
                    let Some(value) = arguments.get(at) else {
                        return self
                            .bind_error(format!("missing required argument {}", parameter.name()));
                    };
                    at += 1;
                    vec![value.clone()]
                }
                ParameterKind::Optional => {
                    let supplied = arguments.get(at);
                    let value = match supplied {
                        Some(JavascriptValue::Undefined) | None => self
                            .policy
                            .defaults
                            .get(parameter.name())
                            .cloned()
                            .or_else(|| supplied.cloned()),
                        Some(value) => Some(value.clone()),
                    };
                    if supplied.is_some() {
                        at += 1;
                    }
                    vec![value.unwrap_or(JavascriptValue::Undefined)]
                }
            };
            bound.insert(parameter.name().clone(), values);
        }
        if at != arguments.len() {
            return self.bind_error("too many arguments".into());
        }
        Ok(bound)
    }

    fn bind_error<T>(&self, message: String) -> Result<T, JavascriptCallError> {
        Err(JavascriptCallError {
            origin: self.policy.error_origin.clone(),
            message,
        })
    }
}

/// Receiver selected for a call.
#[derive(Clone, Debug, PartialEq)]
pub enum JavascriptThis {
    /// Arrow functions retain their lexical receiver.
    Lexical(JavascriptValue),
    /// Ordinary calls receive the call-site receiver.
    Dynamic(JavascriptValue),
}

/// Explicit object-model gaps. These are queryable rather than silent partial
/// emulation of invariants that ordinary objects cannot satisfy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JavascriptObjectGap {
    /// Proxy trap invariants are not emulated by ordinary descriptors.
    ProxyInvariants,
    /// Host and specification exotic internal methods are unsupported.
    ExoticObjectInvariants,
}

/// Error from JavaScript-owned prototype or descriptor policy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JavascriptObjectError {
    /// Shared managed arena rejected an operation.
    Arena(ArenaError),
    /// Shared managed node rejected a checked edge mutation.
    ManagedGraph(JavascriptManagedMutationError),
    /// Descriptor invariant was violated.
    Descriptor(DefineError),
    /// Prototype traversal or interception exceeded its explicit bound.
    Access,
    /// A prototype cycle was requested.
    PrototypeCycle,
    /// `new` was applied to a non-constructor.
    NotConstructor,
    /// A private name was used outside its declaring class.
    PrivateBrand,
}
impl From<ArenaError> for JavascriptObjectError {
    fn from(v: ArenaError) -> Self {
        Self::Arena(v)
    }
}

impl From<JavascriptManagedMutationError> for JavascriptObjectError {
    fn from(value: JavascriptManagedMutationError) -> Self {
        Self::ManagedGraph(value)
    }
}
impl From<DefineError> for JavascriptObjectError {
    fn from(v: DefineError) -> Self {
        Self::Descriptor(v)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Accessor {
    get: Option<JavascriptValue>,
    set: bool,
}
#[derive(Default)]
struct Hooks {
    writes: Vec<(ManagedId, JavascriptPropertyKey, JavascriptValue)>,
}
impl PropertyHook<ManagedId, JavascriptPropertyKey, JavascriptValue, Accessor> for Hooks {
    type Error = ();
    fn get(
        &mut self,
        _: &mut AccessContext<ManagedId, JavascriptPropertyKey>,
        hook: &Accessor,
        _: &ManagedId,
        _: &JavascriptPropertyKey,
    ) -> Result<JavascriptValue, AccessError<()>> {
        hook.get.clone().ok_or(AccessError::Hook(()))
    }
    fn set(
        &mut self,
        _: &mut AccessContext<ManagedId, JavascriptPropertyKey>,
        hook: &Accessor,
        receiver: &ManagedId,
        key: &JavascriptPropertyKey,
        value: JavascriptValue,
    ) -> Result<(), AccessError<()>> {
        if !hook.set {
            return Err(AccessError::Hook(()));
        }
        self.writes.push((*receiver, key.clone(), value));
        Ok(())
    }
}
