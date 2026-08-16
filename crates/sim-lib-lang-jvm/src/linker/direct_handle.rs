/// Access-checked, loader-bound implementation target retained by lambda linkage.
#[derive(Clone, Debug)]
pub struct ResolvedDirectHandle {
    kind: DirectInvocationKind,
    declaring_class: Arc<ClassDefinition>,
    method: JavaMember,
    receiver: DirectReceiver,
}
impl ResolvedDirectHandle {
    /// Exact admitted reference-kind semantics.
    pub const fn kind(&self) -> DirectInvocationKind {
        self.kind
    }
    /// Content- and loader-bound declaration owner.
    pub fn declaring_class(&self) -> &Arc<ClassDefinition> {
        &self.declaring_class
    }
    /// Exact declaration selected by symbolic method resolution.
    pub fn method(&self) -> &JavaMember {
        &self.method
    }
    /// Whether the receiver is absent, captured, or supplied at invocation.
    pub const fn receiver(&self) -> DirectReceiver {
        self.receiver
    }
    /// Whether invocation is an active use that must trigger class initialization.
    pub const fn initializes_on_invocation(&self) -> bool {
        matches!(
            self.kind,
            DirectInvocationKind::Static | DirectInvocationKind::Constructor
        )
    }
}

/// Stable failure stage for resolving a direct lambda implementation handle.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DirectHandleError {
    /// The reference kind is not one of the five invocable direct kinds.
    UnsupportedReferenceKind(u8),
    /// Normative symbolic resolution, including access checking, failed.
    Resolution(ConstantResolutionError),
    /// Managed resolution-cache bookkeeping failed.
    Managed(String),
    /// The resolved constant or declaration contradicts the reference kind.
    KindMismatch,
    /// An instance handle omitted an explicit bound/unbound receiver rule.
    MissingReceiverRule,
    /// A static handle incorrectly carried a receiver.
    UnexpectedReceiver,
}

/// Resolves one implementation handle through the JVM's access-checked method resolver.
///
/// This performs no class initialization. The returned product records whether
/// invocation is an active use, leaving the initialization trigger in the
/// invocation pipeline where JVMS 5.5 requires it.
#[allow(clippy::too_many_arguments)]
pub fn resolve_direct_handle(
    resolution_cache: &ResolutionCache,
    heap: &mut JvmHeap,
    cache_handle: ManagedHandle,
    owner_handle: ManagedHandle,
    loader: &ClassLoader,
    owner: &Arc<ClassDefinition>,
    constant_pool_index: u16,
    reference_kind: u8,
    receiver: DirectReceiver,
) -> Result<ResolvedDirectHandle, DirectHandleError> {
    let kind = match reference_kind {
        5 => DirectInvocationKind::Virtual,
        6 => DirectInvocationKind::Static,
        7 => DirectInvocationKind::Special,
        8 => DirectInvocationKind::Constructor,
        9 => DirectInvocationKind::Interface,
        other => return Err(DirectHandleError::UnsupportedReferenceKind(other)),
    };
    let resolved = resolution_cache
        .resolve(
            heap,
            cache_handle,
            owner_handle,
            loader,
            owner,
            constant_pool_index,
        )
        .map_err(|error: JvmGraphError| DirectHandleError::Managed(format!("{error:?}")))?
        .map_err(DirectHandleError::Resolution)?;
    let expected_constant_kind = if kind == DirectInvocationKind::Interface {
        ConstantResolutionKind::InterfaceMethod
    } else {
        ConstantResolutionKind::Method
    };
    if resolved.kind != expected_constant_kind {
        return Err(DirectHandleError::KindMismatch);
    }
    let declaring_class = loader
        .loaded(resolved.class.binary_name())
        .map_err(|error| DirectHandleError::Managed(error.to_string()))?
        .filter(|class| class.id() == &resolved.class)
        .ok_or(DirectHandleError::KindMismatch)?;
    let name = resolved
        .name
        .as_deref()
        .ok_or(DirectHandleError::KindMismatch)?;
    let descriptor = resolved
        .descriptor
        .as_deref()
        .ok_or(DirectHandleError::KindMismatch)?;
    let method = declaring_class
        .metadata()
        .select_method(name, descriptor)
        .cloned()
        .ok_or(DirectHandleError::KindMismatch)?;
    match kind {
        DirectInvocationKind::Static if receiver != DirectReceiver::None => {
            return Err(DirectHandleError::UnexpectedReceiver);
        }
        DirectInvocationKind::Static if !method.is_static() => {
            return Err(DirectHandleError::KindMismatch);
        }
        DirectInvocationKind::Constructor
            if name != "<init>" || method.is_static() || receiver != DirectReceiver::None =>
        {
            return Err(DirectHandleError::KindMismatch);
        }
        DirectInvocationKind::Special
        | DirectInvocationKind::Virtual
        | DirectInvocationKind::Interface
            if method.is_static() =>
        {
            return Err(DirectHandleError::KindMismatch);
        }
        DirectInvocationKind::Special
        | DirectInvocationKind::Virtual
        | DirectInvocationKind::Interface
            if receiver == DirectReceiver::None =>
        {
            return Err(DirectHandleError::MissingReceiverRule);
        }
        _ => {}
    }
    Ok(ResolvedDirectHandle {
        kind,
        declaring_class,
        method,
        receiver,
    })
}

/// Shape of the variable portion of an admitted lambda bootstrap payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LambdaProtocolTail {
    /// The protocol has exactly its three standard arguments.
    None,
    /// The protocol has flags followed by flag-governed counted sections.
    FlagGoverned,
}

/// One manifest-derived lambda bootstrap identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LambdaBootstrapProtocol {
    /// Bootstrap owner internal name.
    pub owner: &'static str,
    /// Bootstrap member name.
    pub name: &'static str,
    /// Exact bootstrap descriptor.
    pub descriptor: &'static str,
    /// Payload tail shape.
    pub tail: LambdaProtocolTail,
}

struct LambdaBootstrapRegistry {
    protocols: &'static [LambdaBootstrapProtocol],
    admitted_flags_mask: i32,
    reference_kinds: &'static [i64],
}

static LAMBDA_BOOTSTRAP_REGISTRY: LambdaBootstrapRegistry =
    include!(concat!(env!("OUT_DIR"), "/jvm_lambda_protocols.rs"));

/// Returns the executor's manifest-derived admitted lambda protocol set.
pub fn executor_admitted_lambda_protocols() -> &'static [LambdaBootstrapProtocol] {
    LAMBDA_BOOTSTRAP_REGISTRY.protocols
}
