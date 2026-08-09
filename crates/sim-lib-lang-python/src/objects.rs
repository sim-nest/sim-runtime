//! Python object policy over the language-neutral property store.

use sim_lib_dispatch::{
    AccessContext, AccessError, AccessorDescriptor, DataDescriptor, Descriptor, PropertyHook,
    PropertyStore,
};
use std::collections::{BTreeMap, BTreeSet};

/// Value stored by the checked object model.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PythonObjectValue {
    /// Python `None`.
    #[default]
    None,
    /// A bounded integer specimen value.
    Int(i64),
    /// Text.
    String(String),
    /// An object identity.
    Object(u64),
    /// A function identity.
    Function(u64),
    /// A receiver-bound function.
    BoundMethod {
        /// Underlying function identity.
        function: u64,
        /// Bound instance identity.
        receiver: u64,
    },
}

/// Hook token used to prove descriptor receiver behavior.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DescriptorHook {
    /// Stable hook name.
    pub name: String,
    /// Value produced by a getter.
    pub value: PythonObjectValue,
}

/// A declared Python class and its C3-linearized bases.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PythonClass {
    /// Stable class identity.
    pub id: u64,
    /// Display name.
    pub name: String,
    /// Direct bases in declaration order.
    pub bases: Vec<u64>,
    /// C3 method resolution order, including this class.
    pub mro: Vec<u64>,
}

/// Failure to construct a consistent class hierarchy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClassError {
    /// A base class is unknown.
    UnknownBase(u64),
    /// The requested bases have no consistent C3 linearization.
    InconsistentMro,
}

/// Checked attribute failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AttributeError {
    /// The object identity is unknown.
    UnknownObject(u64),
    /// The class identity is unknown.
    UnknownClass(u64),
    /// No attribute was found.
    Missing(String),
    /// Shared descriptor traversal failed.
    Access,
}

#[derive(Default)]
struct Hooks {
    seen_receivers: Vec<u64>,
}
impl PropertyHook<u64, String, PythonObjectValue, DescriptorHook> for Hooks {
    type Error = ();
    fn get(
        &mut self,
        _: &mut AccessContext<u64, String>,
        hook: &DescriptorHook,
        receiver: &u64,
        _: &String,
    ) -> Result<PythonObjectValue, AccessError<Self::Error>> {
        self.seen_receivers.push(*receiver);
        Ok(hook.value.clone())
    }
    fn set(
        &mut self,
        _: &mut AccessContext<u64, String>,
        _: &DescriptorHook,
        receiver: &u64,
        _: &String,
        _: PythonObjectValue,
    ) -> Result<(), AccessError<Self::Error>> {
        self.seen_receivers.push(*receiver);
        Ok(())
    }
}

/// Python class and instance policy using shared property mechanics.
#[derive(Default)]
pub struct PythonObjectSpace {
    classes: BTreeMap<u64, PythonClass>,
    instances: BTreeMap<u64, u64>,
    properties: PropertyStore<u64, String, PythonObjectValue, DescriptorHook>,
    hooks: Hooks,
}

impl PythonObjectSpace {
    /// Declare a class and compute its C3 MRO.
    pub fn define_class(
        &mut self,
        id: u64,
        name: impl Into<String>,
        bases: Vec<u64>,
    ) -> Result<(), ClassError> {
        let mut sequences = Vec::new();
        for base in &bases {
            sequences.push(
                self.classes
                    .get(base)
                    .ok_or(ClassError::UnknownBase(*base))?
                    .mro
                    .clone(),
            );
        }
        sequences.push(bases.clone());
        let mut mro = vec![id];
        mro.extend(c3_merge(sequences)?);
        self.classes.insert(
            id,
            PythonClass {
                id,
                name: name.into(),
                bases,
                mro,
            },
        );
        Ok(())
    }

    /// Allocate an instance of a known class.
    pub fn instantiate(&mut self, object: u64, class: u64) -> Result<(), AttributeError> {
        if !self.classes.contains_key(&class) {
            return Err(AttributeError::UnknownClass(class));
        }
        self.instances.insert(object, class);
        Ok(())
    }

    /// Return a declared class.
    pub fn class(&self, id: u64) -> Option<&PythonClass> {
        self.classes.get(&id)
    }

    /// Define a plain instance or class attribute.
    pub fn define_value(&mut self, owner: u64, key: impl Into<String>, value: PythonObjectValue) {
        self.properties
            .define(
                &owner,
                key.into(),
                Descriptor::Data(DataDescriptor {
                    value,
                    writable: true,
                    enumerable: true,
                    configurable: true,
                }),
            )
            .expect("configurable Python value accepts replacement");
    }

    /// Define a Python data or non-data descriptor on a class.
    pub fn define_descriptor(
        &mut self,
        class: u64,
        key: impl Into<String>,
        getter: DescriptorHook,
        data: bool,
    ) {
        self.properties
            .define(
                &class,
                key.into(),
                Descriptor::Accessor(AccessorDescriptor {
                    get: Some(getter),
                    set: data.then(|| DescriptorHook {
                        name: "set".into(),
                        value: PythonObjectValue::None,
                    }),
                    enumerable: true,
                    configurable: true,
                }),
            )
            .expect("configurable Python descriptor accepts replacement");
    }

    /// Resolve an attribute with Python data-descriptor, instance, non-data,
    /// and class-value precedence.
    pub fn get(&mut self, object: u64, key: &str) -> Result<PythonObjectValue, AttributeError> {
        let class = *self
            .instances
            .get(&object)
            .ok_or(AttributeError::UnknownObject(object))?;
        let mro = self
            .classes
            .get(&class)
            .ok_or(AttributeError::UnknownClass(class))?
            .mro
            .clone();
        let key = key.to_owned();
        let descriptor_owner = mro
            .iter()
            .find(|owner| self.properties.own(owner, &key).is_some())
            .copied();
        if let Some(owner) = descriptor_owner
            && matches!(self.properties.own(&owner, &key), Some(Descriptor::Accessor(a)) if a.set.is_some())
        {
            return self.read(&[owner], object, &key);
        }
        if self.properties.own(&object, &key).is_some() {
            return self.read(&[object], object, &key);
        }
        let value = self.read(&mro, object, &key)?;
        Ok(match value {
            PythonObjectValue::Function(function) => PythonObjectValue::BoundMethod {
                function,
                receiver: object,
            },
            other => other,
        })
    }

    /// Resolve as `super(Current, object)`, starting after `Current` in the MRO.
    pub fn get_super(
        &mut self,
        current: u64,
        object: u64,
        key: &str,
    ) -> Result<PythonObjectValue, AttributeError> {
        let class = *self
            .instances
            .get(&object)
            .ok_or(AttributeError::UnknownObject(object))?;
        let mro = &self
            .classes
            .get(&class)
            .ok_or(AttributeError::UnknownClass(class))?
            .mro;
        let at = mro
            .iter()
            .position(|candidate| *candidate == current)
            .ok_or(AttributeError::UnknownClass(current))?;
        let owners = mro[at + 1..].to_vec();
        let value = self.read(&owners, object, &key.to_owned())?;
        Ok(match value {
            PythonObjectValue::Function(function) => PythonObjectValue::BoundMethod {
                function,
                receiver: object,
            },
            other => other,
        })
    }

    fn read(
        &mut self,
        owners: &[u64],
        receiver: u64,
        key: &String,
    ) -> Result<PythonObjectValue, AttributeError> {
        self.properties
            .get(
                owners,
                &receiver,
                key,
                &mut AccessContext::new(64),
                &mut self.hooks,
            )
            .map_err(|_| AttributeError::Access)?
            .ok_or_else(|| AttributeError::Missing(key.clone()))
    }
}

fn c3_merge(mut sequences: Vec<Vec<u64>>) -> Result<Vec<u64>, ClassError> {
    let mut result = Vec::new();
    loop {
        sequences.retain(|sequence| !sequence.is_empty());
        if sequences.is_empty() {
            return Ok(result);
        }
        let candidate = sequences
            .iter()
            .map(|sequence| sequence[0])
            .find(|candidate| {
                sequences
                    .iter()
                    .all(|sequence| !sequence[1..].contains(candidate))
            })
            .ok_or(ClassError::InconsistentMro)?;
        result.push(candidate);
        for sequence in &mut sequences {
            if sequence.first() == Some(&candidate) {
                sequence.remove(0);
            }
        }
        let unique: BTreeSet<_> = result.iter().copied().collect();
        if unique.len() != result.len() {
            return Err(ClassError::InconsistentMro);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn value(value: i64) -> PythonObjectValue {
        PythonObjectValue::Int(value)
    }
    #[test]
    fn c3_descriptors_binding_and_super_share_property_mechanics() {
        let mut space = PythonObjectSpace::default();
        space.define_class(1, "object", vec![]).unwrap();
        space.define_class(2, "Left", vec![1]).unwrap();
        space.define_class(3, "Right", vec![1]).unwrap();
        space.define_class(4, "Diamond", vec![2, 3]).unwrap();
        assert_eq!(space.class(4).unwrap().mro, vec![4, 2, 3, 1]);
        space.instantiate(10, 4).unwrap();
        space.define_descriptor(
            2,
            "data",
            DescriptorHook {
                name: "data".into(),
                value: value(1),
            },
            true,
        );
        space.define_descriptor(
            2,
            "nondata",
            DescriptorHook {
                name: "nondata".into(),
                value: value(2),
            },
            false,
        );
        space.define_value(10, "data", value(11));
        space.define_value(10, "nondata", value(22));
        assert_eq!(space.get(10, "data"), Ok(value(1)));
        assert_eq!(space.get(10, "nondata"), Ok(value(22)));
        space.define_value(3, "method", PythonObjectValue::Function(7));
        assert_eq!(
            space.get_super(2, 10, "method"),
            Ok(PythonObjectValue::BoundMethod {
                function: 7,
                receiver: 10
            })
        );
        assert_eq!(space.hooks.seen_receivers, vec![10]);
    }

    #[test]
    fn inconsistent_c3_fails_explicitly() {
        let mut space = PythonObjectSpace::default();
        space.define_class(1, "object", vec![]).unwrap();
        space.define_class(2, "A", vec![1]).unwrap();
        space.define_class(3, "B", vec![1]).unwrap();
        space.define_class(4, "AB", vec![2, 3]).unwrap();
        space.define_class(5, "BA", vec![3, 2]).unwrap();
        assert_eq!(
            space.define_class(6, "Impossible", vec![4, 5]),
            Err(ClassError::InconsistentMro)
        );
    }
}
