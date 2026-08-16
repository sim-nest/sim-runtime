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
