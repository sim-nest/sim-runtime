//! Exact, revision-bound identity and state for JVM bootstrap linkage sites.

use std::{collections::BTreeMap, sync::Arc};

use crate::{ClassDefinitionId, ClassSpaceRevision};

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
    use crate::{ClassLoader, JavaClassMetadata};
    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};

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
}
