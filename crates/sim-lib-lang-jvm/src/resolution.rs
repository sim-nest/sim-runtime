//! Lazy, revision-bound JVMS symbolic resolution and access policy.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use sim_codec_classfile::{ClassShell, Constant, ConstantSlot};
use sim_kernel::{Error, Result};

use crate::{
    ClassDefinition, ClassDefinitionId, ClassLoader, ClassLoaderId, ClassSpaceRevision, JavaMember,
    JavaMemberKind, JvmEdge, JvmGraphError, JvmHeap, JvmRole,
};
use sim_lib_mutation::ManagedHandle;

/// Runtime identity of a Java package. A textual name alone is not an identity.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimePackage {
    loader: ClassLoaderId,
    name: String,
}

impl RuntimePackage {
    /// Derives package identity from the defining loader and a binary class name.
    pub fn of(loader: ClassLoaderId, binary_name: &str) -> Self {
        Self {
            loader,
            name: binary_name
                .rsplit_once('.')
                .map_or("", |(name, _)| name)
                .into(),
        }
    }

    /// Defining loader namespace.
    pub const fn loader(&self) -> ClassLoaderId {
        self.loader
    }
    /// Dot-separated package name, or the empty string for the unnamed package.
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Loader-scoped nest identity after `NestHost`/`NestMembers` validation.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RuntimeNest {
    loader: ClassLoaderId,
    host: String,
}

impl RuntimeNest {
    /// Creates the identity established by a validated nest host relationship.
    pub fn new(loader: ClassLoaderId, host_binary_name: impl Into<String>) -> Self {
        Self {
            loader,
            host: host_binary_name.into(),
        }
    }

    /// Defining loader namespace.
    pub const fn loader(&self) -> ClassLoaderId {
        self.loader
    }

    /// Binary name of the nest host.
    pub fn host(&self) -> &str {
        &self.host
    }
}

/// Result of an access-policy check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccessDecision {
    /// Access is permitted.
    Allowed,
    /// Access is denied.
    Denied,
}

impl AccessDecision {
    /// Checks class accessibility under JVMS 5.4.4.
    pub fn class(caller: &ClassDefinition, target: &ClassDefinition) -> Self {
        if target.metadata().access_flags() & 0x0001 != 0
            || RuntimePackage::of(caller.id().loader(), caller.id().binary_name())
                == RuntimePackage::of(target.id().loader(), target.id().binary_name())
        {
            Self::Allowed
        } else {
            Self::Denied
        }
    }

    /// Checks member accessibility, including loader-aware packages and nestmates.
    pub fn member(
        caller: &ClassDefinition,
        declaring: &ClassDefinition,
        member: &JavaMember,
        caller_is_subclass: bool,
        caller_nest: &RuntimeNest,
        declaring_nest: &RuntimeNest,
    ) -> Self {
        let flags = member.access_flags();
        let same_class = caller.id() == declaring.id();
        let same_package = RuntimePackage::of(caller.id().loader(), caller.id().binary_name())
            == RuntimePackage::of(declaring.id().loader(), declaring.id().binary_name());
        if flags & 0x0001 != 0
            || (flags & 0x0002 != 0 && (same_class || caller_nest == declaring_nest))
            || (flags & 0x0004 != 0 && (same_package || caller_is_subclass))
            || (flags & 0x0007 == 0 && same_package)
        {
            Self::Allowed
        } else {
            Self::Denied
        }
    }
}

/// Category of symbolic constant being resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConstantResolutionKind {
    /// `CONSTANT_Class`.
    Class,
    /// `CONSTANT_Fieldref`.
    Field,
    /// `CONSTANT_Methodref`.
    Method,
    /// `CONSTANT_InterfaceMethodref`.
    InterfaceMethod,
}

/// One successfully resolved symbolic constant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstantResolution {
    /// Constant category.
    pub kind: ConstantResolutionKind,
    /// Content-bound target definition identity.
    pub class: ClassDefinitionId,
    /// Member name for member references.
    pub name: Option<String>,
    /// Member descriptor for member references.
    pub descriptor: Option<String>,
}

/// Stable negative outcomes from JVMS symbolic resolution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstantResolutionError {
    /// The index is absent or is not a resolvable symbolic constant.
    InvalidConstant {
        /// Refused pool index.
        index: u16,
    },
    /// The named class has not been defined in this loader namespace.
    NoClassDef {
        /// Binary target name.
        binary_name: String,
    },
    /// Class access failed.
    IllegalAccess {
        /// Inaccessible target class.
        binary_name: String,
    },
    /// The exact field declaration was not found.
    NoSuchField {
        /// Target class.
        binary_name: String,
        /// Field name.
        name: String,
        /// Field descriptor.
        descriptor: String,
    },
    /// The exact method declaration was not found.
    NoSuchMethod {
        /// Target class.
        binary_name: String,
        /// Method name.
        name: String,
        /// Method descriptor.
        descriptor: String,
    },
    /// The constant kind disagrees with whether the target is an interface.
    IncompatibleClassChange {
        /// Target class.
        binary_name: String,
    },
}

#[derive(Clone, Debug)]
pub(crate) enum SymbolicConstant {
    Class {
        binary_name: String,
    },
    Member {
        kind: ConstantResolutionKind,
        binary_name: String,
        name: String,
        descriptor: String,
    },
}

#[derive(Clone)]
struct CacheEntry {
    owner: Weak<ClassDefinition>,
    content_key: u64,
    revision: ClassSpaceRevision,
    outcome: std::result::Result<ConstantResolution, ConstantResolutionError>,
    _managed_value: ManagedHandle,
}

/// Bounded-by-owner symbolic-resolution cache. Weak keys give the same lifetime
/// rule as the managed JVM cache role's ephemerons: entries cannot retain a class.
#[derive(Default)]
pub struct ResolutionCache {
    entries: Mutex<BTreeMap<(ClassDefinitionId, u16), CacheEntry>>,
}

impl ResolutionCache {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves exactly one constant on demand and caches both positive and typed negative results.
    pub fn resolve(
        &self,
        heap: &mut JvmHeap,
        cache: ManagedHandle,
        owner_handle: ManagedHandle,
        loader: &ClassLoader,
        owner: &Arc<ClassDefinition>,
        index: u16,
    ) -> std::result::Result<
        std::result::Result<ConstantResolution, ConstantResolutionError>,
        JvmGraphError,
    > {
        let revision = loader.revision();
        let key = (owner.id().clone(), index);
        let mut entries = self.entries();
        entries.retain(|_, entry| entry.owner.strong_count() != 0);
        if let Some(entry) = entries.get(&key)
            && entry.content_key == owner.id().content_key()
            && entry.revision == revision
        {
            return Ok(entry.outcome.clone());
        }
        let outcome = resolve_uncached(loader, owner, index);
        let managed_value = heap.allocate(JvmRole::Cache).map_err(JvmGraphError::from)?;
        heap.ephemeron(cache, JvmEdge::DerivedEntry, owner_handle, managed_value)?;
        entries.insert(
            key,
            CacheEntry {
                owner: Arc::downgrade(owner),
                content_key: owner.id().content_key(),
                revision,
                outcome: outcome.clone(),
                _managed_value: managed_value,
            },
        );
        Ok(outcome)
    }

    /// Number of live-owner entries; dead ephemeron keys are purged first.
    pub fn live_len(&self) -> usize {
        let mut entries = self.entries();
        entries.retain(|_, entry| entry.owner.strong_count() != 0);
        entries.len()
    }

    fn entries(&self) -> MutexGuard<'_, BTreeMap<(ClassDefinitionId, u16), CacheEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn resolve_uncached(
    loader: &ClassLoader,
    owner: &ClassDefinition,
    index: u16,
) -> std::result::Result<ConstantResolution, ConstantResolutionError> {
    let symbolic = owner
        .resolution_record(index)
        .ok_or(ConstantResolutionError::InvalidConstant { index })?;
    let (kind, binary_name, member) = match symbolic {
        SymbolicConstant::Class { binary_name } => {
            (ConstantResolutionKind::Class, binary_name, None)
        }
        SymbolicConstant::Member {
            kind,
            binary_name,
            name,
            descriptor,
        } => (*kind, binary_name, Some((name, descriptor))),
    };
    let target = loader.loaded(binary_name).ok().flatten().ok_or_else(|| {
        ConstantResolutionError::NoClassDef {
            binary_name: binary_name.clone(),
        }
    })?;
    if AccessDecision::class(owner, &target) == AccessDecision::Denied {
        return Err(ConstantResolutionError::IllegalAccess {
            binary_name: binary_name.clone(),
        });
    }
    let Some((name, descriptor)) = member else {
        return Ok(ConstantResolution {
            kind,
            class: target.id().clone(),
            name: None,
            descriptor: None,
        });
    };
    let target_is_interface = target.metadata().access_flags() & 0x0200 != 0;
    if matches!(kind, ConstantResolutionKind::InterfaceMethod) != target_is_interface
        && kind != ConstantResolutionKind::Field
    {
        return Err(ConstantResolutionError::IncompatibleClassChange {
            binary_name: binary_name.clone(),
        });
    }
    let expected = if kind == ConstantResolutionKind::Field {
        JavaMemberKind::Field
    } else {
        JavaMemberKind::Method
    };
    let declaration = find_member(loader, &target, expected, name, descriptor, &mut Vec::new());
    if let Some((declaring, declaration)) = declaration {
        let caller_nest = RuntimeNest::new(owner.id().loader(), owner.id().binary_name());
        let declaring_nest =
            RuntimeNest::new(declaring.id().loader(), declaring.id().binary_name());
        let caller_is_subclass = owner
            .metadata()
            .resolution()
            .direct_parents()
            .iter()
            .any(|parent| parent == declaring.id().binary_name());
        if AccessDecision::member(
            owner,
            &declaring,
            &declaration,
            caller_is_subclass,
            &caller_nest,
            &declaring_nest,
        ) == AccessDecision::Denied
        {
            return Err(ConstantResolutionError::IllegalAccess {
                binary_name: declaring.id().binary_name().into(),
            });
        }
        Ok(ConstantResolution {
            kind,
            class: declaring.id().clone(),
            name: Some(name.clone()),
            descriptor: Some(descriptor.clone()),
        })
    } else if kind == ConstantResolutionKind::Field {
        Err(ConstantResolutionError::NoSuchField {
            binary_name: binary_name.clone(),
            name: name.clone(),
            descriptor: descriptor.clone(),
        })
    } else {
        Err(ConstantResolutionError::NoSuchMethod {
            binary_name: binary_name.clone(),
            name: name.clone(),
            descriptor: descriptor.clone(),
        })
    }
}

fn find_member(
    loader: &ClassLoader,
    class: &Arc<ClassDefinition>,
    kind: JavaMemberKind,
    name: &str,
    descriptor: &str,
    visited: &mut Vec<ClassDefinitionId>,
) -> Option<(Arc<ClassDefinition>, JavaMember)> {
    if visited.contains(class.id()) {
        return None;
    }
    visited.push(class.id().clone());
    if let Some(member) = class.metadata().members().iter().find(|member| {
        member.kind() == kind && member.name() == name && member.descriptor() == descriptor
    }) {
        return Some((class.clone(), member.clone()));
    }
    class
        .metadata()
        .resolution()
        .direct_parents()
        .iter()
        .filter_map(|parent| loader.loaded(parent).ok().flatten())
        .find_map(|parent| find_member(loader, &parent, kind, name, descriptor, visited))
}

pub(crate) fn symbolic_constants(shell: &ClassShell) -> Result<BTreeMap<u16, SymbolicConstant>> {
    let mut records = BTreeMap::new();
    for (offset, slot) in shell.constant_pool.slots().iter().enumerate() {
        let ConstantSlot::Entry(constant) = slot else {
            continue;
        };
        let index = u16::try_from(offset)
            .map_err(|_| Error::Eval("constant-pool index overflow".into()))?;
        let value = match constant {
            Constant::Class { name_index } => Some(SymbolicConstant::Class {
                binary_name: utf8(shell, *name_index)?.replace('/', "."),
            }),
            Constant::Fieldref {
                class_index,
                name_and_type_index,
            } => Some(member_symbol(
                shell,
                ConstantResolutionKind::Field,
                *class_index,
                *name_and_type_index,
            )?),
            Constant::Methodref {
                class_index,
                name_and_type_index,
            } => Some(member_symbol(
                shell,
                ConstantResolutionKind::Method,
                *class_index,
                *name_and_type_index,
            )?),
            Constant::InterfaceMethodref {
                class_index,
                name_and_type_index,
            } => Some(member_symbol(
                shell,
                ConstantResolutionKind::InterfaceMethod,
                *class_index,
                *name_and_type_index,
            )?),
            _ => None,
        };
        if let Some(value) = value {
            records.insert(index, value);
        }
    }
    Ok(records)
}

fn member_symbol(
    shell: &ClassShell,
    kind: ConstantResolutionKind,
    class_index: u16,
    nat_index: u16,
) -> Result<SymbolicConstant> {
    let Constant::Class { name_index } = entry(shell, class_index)? else {
        return Err(Error::Eval("validated class constant changed type".into()));
    };
    let Constant::NameAndType {
        name_index: member_name,
        descriptor_index,
    } = entry(shell, nat_index)?
    else {
        return Err(Error::Eval("validated name-and-type changed type".into()));
    };
    Ok(SymbolicConstant::Member {
        kind,
        binary_name: utf8(shell, *name_index)?.replace('/', "."),
        name: utf8(shell, *member_name)?,
        descriptor: utf8(shell, *descriptor_index)?,
    })
}

fn entry(shell: &ClassShell, index: u16) -> Result<&Constant> {
    shell
        .constant_pool
        .entry(index, index)
        .map_err(|error| Error::Eval(error.to_string()))
}

fn utf8(shell: &ClassShell, index: u16) -> Result<String> {
    let Constant::Utf8(value) = entry(shell, index)? else {
        return Err(Error::Eval("validated UTF-8 constant changed type".into()));
    };
    String::from_utf16(value.as_code_units())
        .map_err(|_| Error::Eval("class metadata is not valid Unicode".into()))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
    use sim_lib_gc_tracing::CollectionLimits;

    use super::*;
    use crate::JavaClassMetadata;

    fn definition(
        loader: &ClassLoader,
        name: &str,
        key: u64,
        records: BTreeMap<u16, SymbolicConstant>,
    ) -> Arc<ClassDefinition> {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        ClassDefinition::test(
            loader.id(),
            name,
            key,
            JavaClassMetadata::test_identity(&cx, name, &[]),
            records,
        )
    }

    #[test]
    fn packages_include_defining_loader_identity() {
        let first = ClassLoader::new(1);
        let second = ClassLoader::new(1);
        let a = RuntimePackage::of(first.id(), "example.pkg.A");
        let b = RuntimePackage::of(second.id(), "example.pkg.B");
        assert_eq!(a.name(), b.name());
        assert_ne!(a, b);
    }

    #[test]
    fn negative_outcomes_are_revision_bound_ephemerons() {
        let loader = ClassLoader::new(1);
        let records = BTreeMap::from([(
            7,
            SymbolicConstant::Class {
                binary_name: "missing.Target".into(),
            },
        )]);
        let owner = definition(&loader, "example.Owner", 41, records);
        loader.test_insert(owner.clone());
        let cache = ResolutionCache::new();
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
        let cache_handle = heap.allocate(JvmRole::Cache).unwrap();
        let owner_handle = heap.allocate(JvmRole::ClassMirror).unwrap();

        let first = cache
            .resolve(&mut heap, cache_handle, owner_handle, &loader, &owner, 7)
            .unwrap();
        let second = cache
            .resolve(&mut heap, cache_handle, owner_handle, &loader, &owner, 7)
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(cache.live_len(), 1);
        assert_eq!(
            cache.entries().values().next().unwrap().revision,
            loader.revision()
        );

        loader.simulate_class_space_change();
        assert_eq!(
            cache
                .resolve(&mut heap, cache_handle, owner_handle, &loader, &owner, 7)
                .unwrap(),
            first
        );
        assert_eq!(
            cache.entries().values().next().unwrap().revision,
            loader.revision(),
            "a revision bump must replace, not reuse, the cached outcome"
        );

        loader.test_remove("example.Owner");
        drop(owner);
        assert_eq!(cache.live_len(), 0, "a dead class key must clear its entry");
    }
}
