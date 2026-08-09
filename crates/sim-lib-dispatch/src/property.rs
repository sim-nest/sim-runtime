//! Language-neutral own-property storage and bounded descriptor execution.

use std::{collections::HashSet, hash::Hash};

/// A stored data-property descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataDescriptor<V> {
    /// Stored value.
    pub value: V,
    /// Whether assignment may replace the stored value.
    pub writable: bool,
    /// Whether enumeration policy may expose the property.
    pub enumerable: bool,
    /// Whether the property may be deleted or incompatibly redefined.
    pub configurable: bool,
}

/// A stored accessor-property descriptor.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessorDescriptor<H> {
    /// Caller-defined getter hook token.
    pub get: Option<H>,
    /// Caller-defined setter hook token.
    pub set: Option<H>,
    /// Whether enumeration policy may expose the property.
    pub enumerable: bool,
    /// Whether the property may be deleted or incompatibly redefined.
    pub configurable: bool,
}

/// A data or accessor descriptor. The enum prevents mixed invalid records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Descriptor<V, H> {
    /// Directly stored data.
    Data(DataDescriptor<V>),
    /// Caller-interpreted access hooks.
    Accessor(AccessorDescriptor<H>),
}

impl<V, H> Descriptor<V, H> {
    fn configurable(&self) -> bool {
        match self {
            Self::Data(value) => value.configurable,
            Self::Accessor(value) => value.configurable,
        }
    }

    fn enumerable(&self) -> bool {
        match self {
            Self::Data(value) => value.enumerable,
            Self::Accessor(value) => value.enumerable,
        }
    }
}

/// Failure to define or delete an own property.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefineError {
    /// A non-configurable property cannot accept the requested replacement.
    InvariantViolation,
}

/// Kind of guarded accessor invocation.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum AccessKind {
    /// Getter invocation.
    Get,
    /// Setter invocation.
    Set,
}

/// Failure during bounded traversal or accessor interception.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AccessError<E> {
    /// The explicit work budget was exhausted.
    BudgetExhausted,
    /// The same hook was entered recursively for the same receiver and key.
    RecursiveReentry,
    /// A caller-supplied hook failed.
    Hook(E),
}

/// Budget and reentry state shared by one property operation.
pub struct AccessContext<O, K> {
    remaining: usize,
    active: HashSet<(AccessKind, O, K)>,
}

impl<O, K> AccessContext<O, K>
where
    O: Clone + Eq + Hash,
    K: Clone + Eq + Hash,
{
    /// Creates an operation context with an exact work allowance.
    pub fn new(work_budget: usize) -> Self {
        Self {
            remaining: work_budget,
            active: HashSet::new(),
        }
    }

    /// Returns the unspent work allowance.
    pub fn remaining(&self) -> usize {
        self.remaining
    }

    fn charge<E>(&mut self) -> Result<(), AccessError<E>> {
        self.remaining = self
            .remaining
            .checked_sub(1)
            .ok_or(AccessError::BudgetExhausted)?;
        Ok(())
    }

    /// Runs one guarded interception. Nested hooks may use this method to
    /// preserve the same budget and reentry invariant.
    pub fn intercept<T, E>(
        &mut self,
        kind: AccessKind,
        receiver: &O,
        key: &K,
        call: impl FnOnce(&mut Self) -> Result<T, AccessError<E>>,
    ) -> Result<T, AccessError<E>> {
        self.charge()?;
        let signature = (kind, receiver.clone(), key.clone());
        if !self.active.insert(signature.clone()) {
            return Err(AccessError::RecursiveReentry);
        }
        let result = call(self);
        self.active.remove(&signature);
        result
    }
}

/// Caller-owned interpretation of accessor hook tokens.
pub trait PropertyHook<O, K, V, H> {
    /// Hook-specific error.
    type Error;

    /// Invokes a getter with the original receiver, not merely the owner where
    /// the descriptor was found.
    fn get(
        &mut self,
        context: &mut AccessContext<O, K>,
        hook: &H,
        receiver: &O,
        key: &K,
    ) -> Result<V, AccessError<Self::Error>>;

    /// Invokes a setter with the original receiver.
    fn set(
        &mut self,
        context: &mut AccessContext<O, K>,
        hook: &H,
        receiver: &O,
        key: &K,
        value: V,
    ) -> Result<(), AccessError<Self::Error>>;
}

#[derive(Clone, Debug)]
struct OwnProperty<K, V, H> {
    key: K,
    descriptor: Descriptor<V, H>,
}

type PropertyObject<O, K, V, H> = (O, Vec<OwnProperty<K, V, H>>);

/// Ordered own-property records keyed by caller-owned object identities.
///
/// This store deliberately has no parent pointer and no built-in traversal or
/// precedence rule. Callers supply an already-policy-ordered owner slice for
/// every inherited operation.
#[derive(Clone, Debug, Default)]
pub struct PropertyStore<O, K, V, H> {
    objects: Vec<PropertyObject<O, K, V, H>>,
}

impl<O, K, V, H> PropertyStore<O, K, V, H>
where
    O: Clone + Eq + Hash,
    K: Clone + Eq + Hash,
    V: Clone + PartialEq,
    H: Clone + PartialEq,
{
    fn properties(&self, owner: &O) -> Option<&[OwnProperty<K, V, H>]> {
        self.objects
            .iter()
            .find(|(candidate, _)| candidate == owner)
            .map(|(_, properties)| properties.as_slice())
    }

    fn properties_mut(&mut self, owner: &O) -> &mut Vec<OwnProperty<K, V, H>> {
        if let Some(index) = self
            .objects
            .iter()
            .position(|(candidate, _)| candidate == owner)
        {
            return &mut self.objects[index].1;
        }
        self.objects.push((owner.clone(), Vec::new()));
        &mut self.objects.last_mut().expect("object was inserted").1
    }

    /// Returns an own descriptor without invoking it.
    pub fn own(&self, owner: &O, key: &K) -> Option<&Descriptor<V, H>> {
        self.properties(owner)?
            .iter()
            .find(|property| &property.key == key)
            .map(|property| &property.descriptor)
    }

    /// Defines an own property, retaining its original key position on update.
    pub fn define(
        &mut self,
        owner: &O,
        key: K,
        descriptor: Descriptor<V, H>,
    ) -> Result<(), DefineError> {
        let properties = self.properties_mut(owner);
        if let Some(property) = properties.iter_mut().find(|property| property.key == key) {
            if !compatible_redefinition(&property.descriptor, &descriptor) {
                return Err(DefineError::InvariantViolation);
            }
            property.descriptor = descriptor;
        } else {
            properties.push(OwnProperty { key, descriptor });
        }
        Ok(())
    }

    /// Deletes an own property. Missing properties succeed.
    pub fn delete(&mut self, owner: &O, key: &K) -> Result<bool, DefineError> {
        let Some((_, properties)) = self
            .objects
            .iter_mut()
            .find(|(candidate, _)| candidate == owner)
        else {
            return Ok(false);
        };
        let Some(index) = properties.iter().position(|property| &property.key == key) else {
            return Ok(false);
        };
        if !properties[index].descriptor.configurable() {
            return Err(DefineError::InvariantViolation);
        }
        properties.remove(index);
        Ok(true)
    }

    /// Returns own keys in stable definition order, optionally filtering out
    /// non-enumerable records. Delete followed by define appends a fresh key.
    pub fn own_keys(&self, owner: &O, enumerable_only: bool) -> Vec<K> {
        self.properties(owner)
            .unwrap_or_default()
            .iter()
            .filter(|property| !enumerable_only || property.descriptor.enumerable())
            .map(|property| property.key.clone())
            .collect()
    }

    /// Reads along an explicitly supplied owner order. Duplicate owners are
    /// skipped, making cyclic caller-produced orders safe.
    pub fn get<E>(
        &self,
        owners: &[O],
        receiver: &O,
        key: &K,
        context: &mut AccessContext<O, K>,
        hooks: &mut impl PropertyHook<O, K, V, H, Error = E>,
    ) -> Result<Option<V>, AccessError<E>> {
        let mut visited = HashSet::new();
        for owner in owners {
            context.charge()?;
            if !visited.insert(owner.clone()) {
                continue;
            }
            let Some(descriptor) = self.own(owner, key) else {
                continue;
            };
            return match descriptor {
                Descriptor::Data(data) => Ok(Some(data.value.clone())),
                Descriptor::Accessor(accessor) => match &accessor.get {
                    Some(hook) => context
                        .intercept(AccessKind::Get, receiver, key, |context| {
                            hooks.get(context, hook, receiver, key)
                        })
                        .map(Some),
                    None => Ok(None),
                },
            };
        }
        Ok(None)
    }

    /// Assigns through the first descriptor in an explicit owner order.
    /// Writable data is updated on its owner; accessor setters receive the
    /// original receiver. Missing and read-only properties return `Ok(false)`.
    pub fn set<E>(
        &mut self,
        owners: &[O],
        receiver: &O,
        key: &K,
        value: V,
        context: &mut AccessContext<O, K>,
        hooks: &mut impl PropertyHook<O, K, V, H, Error = E>,
    ) -> Result<bool, AccessError<E>> {
        let mut visited = HashSet::new();
        for owner in owners {
            context.charge()?;
            if !visited.insert(owner.clone()) {
                continue;
            }
            let Some(descriptor) = self.own(owner, key).cloned() else {
                continue;
            };
            return match descriptor {
                Descriptor::Data(data) if data.writable => {
                    let property = self
                        .properties_mut(owner)
                        .iter_mut()
                        .find(|property| &property.key == key)
                        .expect("descriptor was found");
                    let Descriptor::Data(data) = &mut property.descriptor else {
                        unreachable!("cloned descriptor kind remains stable")
                    };
                    data.value = value;
                    Ok(true)
                }
                Descriptor::Data(_) => Ok(false),
                Descriptor::Accessor(accessor) => match accessor.set {
                    Some(hook) => context
                        .intercept(AccessKind::Set, receiver, key, |context| {
                            hooks.set(context, &hook, receiver, key, value)
                        })
                        .map(|()| true),
                    None => Ok(false),
                },
            };
        }
        Ok(false)
    }
}

fn compatible_redefinition<V: PartialEq, H: PartialEq>(
    current: &Descriptor<V, H>,
    replacement: &Descriptor<V, H>,
) -> bool {
    if current.configurable() {
        return true;
    }
    match (current, replacement) {
        (Descriptor::Data(old), Descriptor::Data(new)) => {
            !new.configurable
                && old.enumerable == new.enumerable
                && (old.writable || !new.writable)
                && (old.writable || old.value == new.value)
        }
        (Descriptor::Accessor(old), Descriptor::Accessor(new)) => {
            !new.configurable
                && old.enumerable == new.enumerable
                && old.get == new.get
                && old.set == new.set
        }
        _ => false,
    }
}
