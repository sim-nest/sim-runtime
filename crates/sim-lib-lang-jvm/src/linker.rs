//! Exact, revision-bound identity and state for JVM bootstrap linkage sites.

use std::{collections::BTreeMap, sync::Arc};

use sim_lib_mutation::ManagedHandle;

use crate::{
    ClassDefinition, ClassDefinitionId, ClassLoader, ClassSpaceRevision, ConstantResolutionError,
    ConstantResolutionKind, JavaMember, JvmGraphError, JvmHeap, ResolutionCache,
};

/// Receiver placement retained by a resolved direct implementation handle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectReceiver {
    /// A static implementation or constructor has no pre-existing receiver.
    None,
    /// The receiver is captured when the lambda instance is created.
    Bound,
    /// The receiver is supplied as the first SAM invocation argument.
    Unbound,
}

/// Exact invocation semantics of an admitted `CONSTANT_MethodHandle` kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DirectInvocationKind {
    /// `REF_invokeStatic` (6).
    Static,
    /// `REF_newInvokeSpecial` (8).
    Constructor,
    /// `REF_invokeSpecial` (7).
    Special,
    /// `REF_invokeVirtual` (5).
    Virtual,
    /// `REF_invokeInterface` (9).
    Interface,
}

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

/// A resolved constant-pool bootstrap argument ready for protocol validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedBootstrapArgument {
    /// A method-type descriptor.
    MethodType(String),
    /// A method handle and its JVMS reference kind.
    MethodHandle {
        /// JVMS `reference_kind` from the resolved `CONSTANT_MethodHandle`.
        reference_kind: u8,
    },
    /// A marker-interface class internal name.
    Class(String),
    /// An integer flag or count.
    Integer(i32),
}

/// Fully validated lambda bootstrap payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LambdaBootstrapPlan {
    /// Erased SAM method descriptor.
    pub sam_method_type: String,
    /// Implementation method-handle reference kind.
    pub implementation_reference_kind: u8,
    /// Instantiated SAM method descriptor.
    pub instantiated_method_type: String,
    /// Marker interfaces requested by `altMetafactory`.
    pub marker_interfaces: Vec<String>,
    /// Additional bridge method descriptors.
    pub bridges: Vec<String>,
    /// Whether serialization support was requested.
    pub serializable: bool,
}

/// Fail-closed lambda bootstrap admission or payload error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LambdaBootstrapError {
    /// Bootstrap identity is absent from the shared registry.
    UnadmittedProtocol,
    /// Payload length, kind, flags, count, or descriptor is malformed.
    MalformedPayload(String),
    /// Implementation handle is not an invocable method handle.
    UnadmittedReferenceKind(u8),
}

/// Decodes and validates a lambda bootstrap before any generated class or instance is allocated.
pub fn decode_lambda_bootstrap(
    owner: &str,
    name: &str,
    descriptor: &str,
    arguments: &[ResolvedBootstrapArgument],
) -> Result<LambdaBootstrapPlan, LambdaBootstrapError> {
    let protocol = LAMBDA_BOOTSTRAP_REGISTRY
        .protocols
        .iter()
        .find(|protocol| {
            protocol.owner == owner && protocol.name == name && protocol.descriptor == descriptor
        })
        .ok_or(LambdaBootstrapError::UnadmittedProtocol)?;
    let method_type = |index: usize| match arguments.get(index) {
        Some(ResolvedBootstrapArgument::MethodType(value)) if valid_method_descriptor(value) => {
            Ok(value.clone())
        }
        _ => Err(LambdaBootstrapError::MalformedPayload(format!(
            "argument {index} must be a valid MethodType"
        ))),
    };
    let sam_method_type = method_type(0)?;
    let implementation_reference_kind = match arguments.get(1) {
        Some(ResolvedBootstrapArgument::MethodHandle { reference_kind }) => {
            if LAMBDA_BOOTSTRAP_REGISTRY
                .reference_kinds
                .contains(&i64::from(*reference_kind))
            {
                *reference_kind
            } else {
                return Err(LambdaBootstrapError::UnadmittedReferenceKind(
                    *reference_kind,
                ));
            }
        }
        _ => {
            return Err(LambdaBootstrapError::MalformedPayload(
                "argument 1 must be a MethodHandle".into(),
            ));
        }
    };
    let instantiated_method_type = method_type(2)?;
    let mut marker_interfaces = Vec::new();
    let mut bridges = Vec::new();
    let mut serializable = false;
    match protocol.tail {
        LambdaProtocolTail::None if arguments.len() != 3 => {
            return Err(LambdaBootstrapError::MalformedPayload(
                "metafactory requires exactly 3 arguments".into(),
            ));
        }
        LambdaProtocolTail::None => {}
        LambdaProtocolTail::FlagGoverned => {
            let flags = match arguments.get(3) {
                Some(ResolvedBootstrapArgument::Integer(flags))
                    if *flags >= 0
                        && flags & !LAMBDA_BOOTSTRAP_REGISTRY.admitted_flags_mask == 0 =>
                {
                    *flags
                }
                _ => {
                    return Err(LambdaBootstrapError::MalformedPayload(
                        "altMetafactory requires admitted flags".into(),
                    ));
                }
            };
            serializable = flags & 1 != 0;
            let mut cursor = 4;
            if flags & 2 != 0 {
                let count = payload_count(arguments, &mut cursor, "marker")?;
                for _ in 0..count {
                    match arguments.get(cursor) {
                        Some(ResolvedBootstrapArgument::Class(name)) if !name.is_empty() => {
                            marker_interfaces.push(name.clone())
                        }
                        _ => {
                            return Err(LambdaBootstrapError::MalformedPayload(
                                "marker must be a class".into(),
                            ));
                        }
                    }
                    cursor += 1;
                }
            }
            if flags & 4 != 0 {
                let count = payload_count(arguments, &mut cursor, "bridge")?;
                for _ in 0..count {
                    bridges.push(method_type(cursor)?);
                    cursor += 1;
                }
            }
            if cursor != arguments.len() {
                return Err(LambdaBootstrapError::MalformedPayload(
                    "trailing altMetafactory arguments".into(),
                ));
            }
        }
    }
    Ok(LambdaBootstrapPlan {
        sam_method_type,
        implementation_reference_kind,
        instantiated_method_type,
        marker_interfaces,
        bridges,
        serializable,
    })
}

fn payload_count(
    arguments: &[ResolvedBootstrapArgument],
    cursor: &mut usize,
    label: &str,
) -> Result<usize, LambdaBootstrapError> {
    let count = match arguments.get(*cursor) {
        Some(ResolvedBootstrapArgument::Integer(value)) if *value >= 0 => {
            usize::try_from(*value).unwrap()
        }
        _ => {
            return Err(LambdaBootstrapError::MalformedPayload(format!(
                "{label} count is missing or negative"
            )));
        }
    };
    *cursor += 1;
    if count > arguments.len().saturating_sub(*cursor) {
        return Err(LambdaBootstrapError::MalformedPayload(format!(
            "{label} payload is truncated"
        )));
    }
    Ok(count)
}

fn valid_method_descriptor(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.first() != Some(&b'(') {
        return false;
    }
    let mut cursor = 1;
    while bytes.get(cursor) != Some(&b')') {
        if !parse_descriptor_type(bytes, &mut cursor, false) {
            return false;
        }
    }
    cursor += 1;
    if bytes.get(cursor) == Some(&b'V') {
        cursor += 1;
    } else if !parse_descriptor_type(bytes, &mut cursor, false) {
        return false;
    }
    cursor == bytes.len()
}

fn parse_descriptor_type(bytes: &[u8], cursor: &mut usize, in_array: bool) -> bool {
    match bytes.get(*cursor).copied() {
        Some(b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z') => {
            *cursor += 1;
            true
        }
        Some(b'L') => {
            *cursor += 1;
            let start = *cursor;
            while !matches!(bytes.get(*cursor), None | Some(b';')) {
                *cursor += 1;
            }
            if *cursor == start || bytes.get(*cursor) != Some(&b';') {
                return false;
            }
            *cursor += 1;
            true
        }
        Some(b'[') if !in_array => {
            while bytes.get(*cursor) == Some(&b'[') {
                *cursor += 1;
            }
            parse_descriptor_type(bytes, cursor, true)
        }
        _ => false,
    }
}

/// Identity of the method enclosing a bootstrap occurrence.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct MethodIdentity {
    /// JVM method name.
    pub name: String,
    /// JVM method descriptor.
    pub descriptor: String,
}

/// One raw constant-pool bootstrap argument, before protocol interpretation.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub enum BootstrapArgument {
    /// Constant-pool index of a loadable constant.
    Constant(u16),
    /// Exact integer payload.
    Integer(i32),
    /// Exact long payload.
    Long(i64),
    /// Exact IEEE-754 single payload.
    FloatBits(u32),
    /// Exact IEEE-754 double payload.
    DoubleBits(u64),
    /// Exact modified-UTF-8-decoded string code units.
    String(Box<[u16]>),
}

/// Raw decoded `BootstrapMethods` record retained without protocol assumptions.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct BootstrapMethod {
    /// Constant-pool index of the bootstrap method handle.
    pub method_handle: u16,
    /// Bootstrap arguments in classfile order.
    pub arguments: Box<[BootstrapArgument]>,
}

/// Immutable identity of one bootstrap instruction occurrence.
///
/// `class` carries loader, binary-name, and classfile-content identity. The
/// constant-pool index deliberately remains part of the key even when every
/// decoded bootstrap argument is byte-for-byte identical to another site.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Hash)]
pub struct SiteKey {
    /// Exact defining class, including loader and classfile content identity.
    pub class: ClassDefinitionId,
    /// Method containing the instruction occurrence.
    pub method: MethodIdentity,
    /// Constant-pool index named by this instruction occurrence.
    pub constant_pool_index: u16,
    /// Raw decoded bootstrap record selected by the dynamic constant.
    pub bootstrap: BootstrapMethod,
}

/// Typed, cacheable bootstrap linkage failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LinkageFailure {
    /// A referenced constant-pool entry is absent or has the wrong kind.
    InvalidConstantPoolEntry(u16),
    /// The bootstrap protocol is not admitted by the installed linker.
    UnsupportedBootstrap {
        /// Refused bootstrap owner.
        owner: String,
        /// Refused bootstrap member name.
        name: String,
    },
    /// The dynamic invocation descriptor is malformed.
    InvalidDescriptor(String),
    /// Bootstrap execution failed with a stable JVM linkage condition.
    Bootstrap(String),
}

/// Revision-bound state of one linkage occurrence.
#[derive(Debug, Eq, PartialEq)]
pub enum LinkageState<T> {
    /// The site has not yet been linked at this revision.
    Unlinked,
    /// Successful immutable linkage product.
    Linked(Arc<T>),
    /// Stable typed failure produced while linking.
    Failed(LinkageFailure),
}

impl<T> Clone for LinkageState<T> {
    fn clone(&self) -> Self {
        match self {
            Self::Unlinked => Self::Unlinked,
            Self::Linked(value) => Self::Linked(Arc::clone(value)),
            Self::Failed(error) => Self::Failed(error.clone()),
        }
    }
}

#[derive(Clone, Debug)]
struct CacheEntry<T> {
    revision: ClassSpaceRevision,
    state: LinkageState<T>,
}

/// Per-occurrence cache whose successes and failures expire together on a
/// class-space revision change.
#[derive(Clone, Debug, Default)]
pub struct LinkageCache<T> {
    entries: BTreeMap<SiteKey, CacheEntry<T>>,
}

impl<T> LinkageCache<T> {
    /// Creates an empty linkage cache.
    pub const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    /// Returns the current state, treating an entry from another loader
    /// revision as unlinked rather than exposing stale linkage.
    pub fn state(&self, key: &SiteKey, revision: ClassSpaceRevision) -> LinkageState<T> {
        self.entries
            .get(key)
            .filter(|entry| entry.revision == revision)
            .map_or(LinkageState::Unlinked, |entry| entry.state.clone())
    }

    /// Links once per exact site and revision, caching typed failures as well as
    /// successful products. A stale entry is replaced only after `link` runs.
    pub fn resolve<F>(
        &mut self,
        key: SiteKey,
        revision: ClassSpaceRevision,
        link: F,
    ) -> Result<Arc<T>, LinkageFailure>
    where
        F: FnOnce() -> Result<T, LinkageFailure>,
    {
        if let Some(entry) = self
            .entries
            .get(&key)
            .filter(|entry| entry.revision == revision)
        {
            return match &entry.state {
                LinkageState::Linked(value) => Ok(Arc::clone(value)),
                LinkageState::Failed(error) => Err(error.clone()),
                LinkageState::Unlinked => unreachable!("unlinked states are not stored"),
            };
        }
        let state = match link() {
            Ok(value) => LinkageState::Linked(Arc::new(value)),
            Err(error) => LinkageState::Failed(error),
        };
        self.entries.insert(
            key,
            CacheEntry {
                revision,
                state: state.clone(),
            },
        );
        match state {
            LinkageState::Linked(value) => Ok(value),
            LinkageState::Failed(error) => Err(error),
            LinkageState::Unlinked => unreachable!("unlinked states are not stored"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClassLoader, JavaClassMetadata, JvmRole, resolution::SymbolicConstant};
    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
    use sim_lib_gc_tracing::CollectionLimits;

    fn fixture() -> (SiteKey, ClassLoader) {
        let loader = ClassLoader::new(4096);
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let class = crate::ClassDefinition::test(
            loader.id(),
            "Example",
            0x51_7e,
            JavaClassMetadata::test_identity(&cx, "Example", &[]),
            BTreeMap::new(),
        );
        (
            SiteKey {
                class: class.id().clone(),
                method: MethodIdentity {
                    name: "make".into(),
                    descriptor: "()V".into(),
                },
                constant_pool_index: 7,
                bootstrap: BootstrapMethod {
                    method_handle: 3,
                    arguments: vec![BootstrapArgument::Constant(11)].into_boxed_slice(),
                },
            },
            loader,
        )
    }

    #[test]
    fn identical_lambdas_at_two_occurrences_are_distinct_sites() {
        let (first, loader) = fixture();
        let mut second = first.clone();
        second.constant_pool_index = 8;
        assert_ne!(first, second);
        let mut cache = LinkageCache::new();
        let revision = loader.revision();
        let first_value = cache
            .resolve(first, revision, || Ok::<_, LinkageFailure>("first"))
            .unwrap();
        let second_value = cache
            .resolve(second, revision, || Ok::<_, LinkageFailure>("second"))
            .unwrap();
        assert_eq!((*first_value, *second_value), ("first", "second"));
    }

    #[test]
    fn revision_bump_relinks_cached_success_and_failure() {
        let (key, loader) = fixture();
        let revision = loader.revision();
        loader.simulate_class_space_change();
        let next = loader.revision();
        let mut successes = LinkageCache::new();
        let original = successes
            .resolve(key.clone(), revision, || Ok::<_, LinkageFailure>(1))
            .unwrap();
        let relinked = successes
            .resolve(key.clone(), next, || Ok::<_, LinkageFailure>(2))
            .unwrap();
        assert_eq!((*original, *relinked), (1, 2));

        let mut failures = LinkageCache::<u8>::new();
        let stale = LinkageFailure::Bootstrap("stale".into());
        assert_eq!(
            failures.resolve(key.clone(), revision, || Err(stale.clone())),
            Err(stale)
        );
        assert_eq!(*failures.resolve(key, next, || Ok(9)).unwrap(), 9);
    }

    fn direct_fixture(
        target_flags: u16,
        method_flags: u16,
    ) -> (
        ClassLoader,
        Arc<ClassDefinition>,
        JvmHeap,
        ManagedHandle,
        ManagedHandle,
    ) {
        let loader = ClassLoader::new(4096);
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let target = ClassDefinition::test(
            loader.id(),
            "target.Target",
            2,
            JavaClassMetadata::test_class(
                &cx,
                "target.Target",
                &[],
                target_flags,
                &[("run", "()V", method_flags)],
            ),
            BTreeMap::new(),
        );
        let owner = ClassDefinition::test(
            loader.id(),
            "caller.Owner",
            1,
            JavaClassMetadata::test_class(&cx, "caller.Owner", &[], 0x0001, &[]),
            BTreeMap::from([(
                7,
                SymbolicConstant::Member {
                    kind: ConstantResolutionKind::Method,
                    binary_name: "target.Target".into(),
                    name: "run".into(),
                    descriptor: "()V".into(),
                },
            )]),
        );
        loader.test_insert(target);
        loader.test_insert(owner.clone());
        let mut heap = JvmHeap::new(
            8,
            CollectionLimits {
                objects: 8,
                edges: 8,
                stack: 8,
                work: 32,
                clears: 8,
                finalizers: 0,
            },
        )
        .unwrap();
        let cache = heap.allocate(JvmRole::Cache).unwrap();
        let owner_handle = heap.allocate(JvmRole::ClassMirror).unwrap();
        (loader, owner, heap, cache, owner_handle)
    }

    #[test]
    fn static_direct_handle_defers_initialization_until_invocation() {
        let (loader, owner, mut heap, cache, owner_handle) = direct_fixture(0x0001, 0x0009);
        let handle = resolve_direct_handle(
            &ResolutionCache::new(),
            &mut heap,
            cache,
            owner_handle,
            &loader,
            &owner,
            7,
            6,
            DirectReceiver::None,
        )
        .unwrap();
        assert_eq!(handle.kind(), DirectInvocationKind::Static);
        assert!(handle.initializes_on_invocation());
        assert_eq!(handle.declaring_class().id().loader(), loader.id());
    }

    #[test]
    fn inaccessible_direct_target_fails_during_normative_resolution() {
        let (loader, owner, mut heap, cache, owner_handle) = direct_fixture(0, 0x0009);
        assert_eq!(
            resolve_direct_handle(
                &ResolutionCache::new(),
                &mut heap,
                cache,
                owner_handle,
                &loader,
                &owner,
                7,
                6,
                DirectReceiver::None,
            )
            .unwrap_err(),
            DirectHandleError::Resolution(ConstantResolutionError::IllegalAccess {
                binary_name: "target.Target".into(),
            })
        );
    }

    #[test]
    fn receiver_rules_and_unsupported_kinds_fail_closed() {
        let (loader, owner, mut heap, cache, owner_handle) = direct_fixture(0x0001, 0x0001);
        assert!(matches!(
            resolve_direct_handle(
                &ResolutionCache::new(),
                &mut heap,
                cache,
                owner_handle,
                &loader,
                &owner,
                7,
                5,
                DirectReceiver::None,
            ),
            Err(DirectHandleError::MissingReceiverRule)
        ));
        assert!(matches!(
            resolve_direct_handle(
                &ResolutionCache::new(),
                &mut heap,
                cache,
                owner_handle,
                &loader,
                &owner,
                7,
                1,
                DirectReceiver::None,
            ),
            Err(DirectHandleError::UnsupportedReferenceKind(1))
        ));
    }
}
