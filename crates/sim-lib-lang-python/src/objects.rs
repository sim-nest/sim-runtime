//! Python object policy over the language-neutral property store.

use sim_kernel::{
    ClassId, ClassRef, Cx, Expr, MatchScore, Shape, ShapeDoc, ShapeMatch, ShapeRef, Value,
};
use sim_lib_class::{
    C3Policy, CacheError, ClassCache, ClassDescriptor, ClassDescriptorInput, ClassIdentity,
    ClassRoot, DeclaredParent, LineageBudget,
};
use sim_lib_dispatch::{
    AccessContext, AccessError, AccessorDescriptor, DataDescriptor, Descriptor, PropertyHook,
    PropertyStore,
};
use sim_lib_gc_tracing::{CollectionLimits, CollectionReceipt};
use std::{collections::BTreeMap, sync::Arc};

// conformance: Python object policy checks classes, descriptors, methods, and super.

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
#[derive(Clone, Debug)]
pub struct PythonClass {
    /// Runtime class identity. Python never substitutes an integer surrogate.
    pub identity: ClassRef,
    /// Display name.
    pub name: String,
    /// Direct bases in declaration order.
    pub descriptor: ClassDescriptor,
    /// C3 method resolution order, including this class.
    pub mro: Vec<ClassRef>,
    cache_root: ClassRoot,
}

/// Failure to construct a consistent class hierarchy.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClassError {
    /// A base class is unknown.
    UnknownBase(ClassId),
    /// The supplied value is not a class or its descriptor is malformed.
    InvalidClass,
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
pub struct PythonObjectSpace {
    classes: BTreeMap<ClassId, PythonClass>,
    cache_ids: BTreeMap<sim_lib_mutation::ManagedId, ClassId>,
    cache: ClassCache<()>,
    instances: BTreeMap<u64, ClassId>,
    properties: PropertyStore<u64, String, PythonObjectValue, DescriptorHook>,
    hooks: Hooks,
}

impl Default for PythonObjectSpace {
    fn default() -> Self {
        Self {
            classes: BTreeMap::new(),
            cache_ids: BTreeMap::new(),
            cache: ClassCache::new(4096).expect("fixed positive Python class-cache capacity"),
            instances: BTreeMap::new(),
            properties: PropertyStore::default(),
            hooks: Hooks::default(),
        }
    }
}

struct PythonAnyShape;
impl Shape for PythonAnyShape {
    fn check_value(&self, _: &mut Cx, _: Value) -> sim_kernel::Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(1)))
    }
    fn check_expr(&self, _: &mut Cx, _: &Expr) -> sim_kernel::Result<ShapeMatch> {
        Ok(ShapeMatch::accept(MatchScore::exact(1)))
    }
    fn describe(&self, _: &mut Cx) -> sim_kernel::Result<ShapeDoc> {
        Ok(ShapeDoc::new("python-object"))
    }
}

impl PythonObjectSpace {
    pub(crate) fn is_subclass(&self, class: ClassId, candidate: ClassId) -> bool {
        self.classes.get(&class).is_some_and(|declared| {
            declared.mro.iter().any(|entry| {
                entry
                    .object()
                    .as_class()
                    .is_some_and(|entry| entry.id() == candidate)
            })
        })
    }

    pub(crate) fn subclass_work(&self, class: ClassId, candidate: ClassId, limit: usize) -> usize {
        let Some(declared) = self.classes.get(&class) else {
            return 0;
        };
        let required = declared
            .mro
            .iter()
            .position(|entry| {
                entry
                    .object()
                    .as_class()
                    .is_some_and(|entry| entry.id() == candidate)
            })
            .map_or(declared.mro.len(), |at| at + 1);
        if required > limit {
            limit.saturating_add(1)
        } else {
            required
        }
    }

    /// Declare a class and compute its C3 MRO.
    pub fn define_class(
        &mut self,
        cx: &Cx,
        identity: ClassRef,
        bases: Vec<ClassRef>,
    ) -> Result<(), ClassError> {
        let class = identity
            .object()
            .as_class()
            .ok_or(ClassError::InvalidClass)?;
        let id = class.id();
        let name = class.symbol().name.to_string();
        let mut parent_roots = Vec::with_capacity(bases.len());
        let mut parents = Vec::with_capacity(bases.len());
        for base in &bases {
            let parent = base.object().as_class().ok_or(ClassError::InvalidClass)?;
            let stored = self
                .classes
                .get(&parent.id())
                .ok_or(ClassError::UnknownBase(parent.id()))?;
            parent_roots.push(stored.cache_root);
            parents.push(DeclaredParent::resolved(
                ClassIdentity::checked(parent.id(), parent.symbol().clone())
                    .map_err(|_| ClassError::InvalidClass)?,
                base.clone(),
            ));
        }
        let shape: ShapeRef = cx
            .factory()
            .opaque(Arc::new(PythonAnyShape))
            .map_err(|_| ClassError::InvalidClass)?;
        let descriptor = ClassDescriptor::new(ClassDescriptorInput {
            identity: ClassIdentity::checked(id, class.symbol().clone())
                .map_err(|_| ClassError::InvalidClass)?,
            parents,
            constructor_shape: shape.clone(),
            instance_shape: shape,
            members: Vec::new(),
            read_construction: None,
            metadata: Vec::new(),
        })
        .map_err(|_| ClassError::InconsistentMro)?;
        let cache_root = self
            .cache
            .allocate_class(&parent_roots, Vec::new())
            .map_err(map_cache_error)?;
        self.cache_ids.insert(cache_root.id(), id);
        let derived = match self.cache.derived(cache_root, &C3Policy, lineage_budget()) {
            Ok(derived) => derived,
            Err(error) => {
                self.cache_ids.remove(&cache_root.id());
                self.cache.release(cache_root).map_err(map_cache_error)?;
                return Err(map_cache_error(error));
            }
        };
        let mro = derived
            .view
            .linearization
            .iter()
            .map(|managed| {
                let class_id = self.cache_ids[managed];
                if class_id == id {
                    identity.clone()
                } else {
                    self.classes[&class_id].identity.clone()
                }
            })
            .collect();
        self.classes.insert(
            id,
            PythonClass {
                identity,
                name,
                descriptor,
                mro,
                cache_root,
            },
        );
        Ok(())
    }

    /// Allocate an instance of a known class.
    pub fn instantiate(&mut self, object: u64, class: ClassRef) -> Result<(), AttributeError> {
        let id = class
            .object()
            .as_class()
            .ok_or(AttributeError::UnknownClass(u64::MAX))?
            .id();
        if !self.classes.contains_key(&id) {
            return Err(AttributeError::UnknownClass(u64::from(id.0)));
        }
        self.instances.insert(object, id);
        Ok(())
    }

    /// Return a declared class.
    pub fn class(&self, id: ClassId) -> Option<&PythonClass> {
        self.classes.get(&id)
    }

    /// Releases a declared class root so its shared derived MRO can be reclaimed.
    pub fn release_class(&mut self, id: ClassId) -> Result<(), ClassError> {
        let class = self
            .classes
            .remove(&id)
            .ok_or(ClassError::UnknownBase(id))?;
        self.cache
            .release(class.cache_root)
            .map_err(map_cache_error)
    }

    /// Collects unreachable class descriptors and ephemeron-owned derived MRO values.
    pub fn collect_classes(
        &mut self,
        limits: CollectionLimits,
    ) -> Result<CollectionReceipt, ClassError> {
        self.cache.collect(limits).map_err(map_cache_error)
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
            .ok_or(AttributeError::UnknownClass(u64::from(class.0)))?
            .mro
            .iter()
            .map(class_id)
            .collect::<Result<Vec<_>, _>>()?;
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
        let mro = self
            .classes
            .get(&class)
            .ok_or(AttributeError::UnknownClass(u64::from(class.0)))?
            .mro
            .iter()
            .map(class_id)
            .collect::<Result<Vec<_>, _>>()?;
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

fn lineage_budget() -> LineageBudget {
    LineageBudget {
        nodes: 4096,
        work: 1_000_000,
    }
}
fn map_cache_error(error: CacheError) -> ClassError {
    match error {
        CacheError::Lineage(_) => ClassError::InconsistentMro,
        _ => ClassError::InvalidClass,
    }
}
fn class_id(class: &ClassRef) -> Result<u64, AttributeError> {
    class
        .object()
        .as_class()
        .map(|class| u64::from(class.id().0))
        .ok_or(AttributeError::Access)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_kernel::Symbol;
    fn class(cx: &Cx, id: u32, name: &str) -> ClassRef {
        cx.factory()
            .class_stub(ClassId(id), Symbol::qualified("python", name))
            .unwrap()
    }
    fn value(value: i64) -> PythonObjectValue {
        PythonObjectValue::Int(value)
    }
    #[test]
    fn c3_descriptors_binding_and_super_share_property_mechanics() {
        let mut space = PythonObjectSpace::default();
        let cx = sim_kernel::testing::bare_cx();
        let object = class(&cx, 1, "object");
        let left = class(&cx, 2, "Left");
        let right = class(&cx, 3, "Right");
        let diamond = class(&cx, 4, "Diamond");
        space.define_class(&cx, object.clone(), vec![]).unwrap();
        space
            .define_class(&cx, left.clone(), vec![object.clone()])
            .unwrap();
        space
            .define_class(&cx, right.clone(), vec![object.clone()])
            .unwrap();
        space
            .define_class(&cx, diamond.clone(), vec![left.clone(), right.clone()])
            .unwrap();
        assert_eq!(
            space.class(ClassId(4)).unwrap().mro,
            vec![diamond.clone(), left, right, object]
        );
        space.instantiate(10, diamond).unwrap();
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
        let cx = sim_kernel::testing::bare_cx();
        let object = class(&cx, 1, "object");
        let a = class(&cx, 2, "A");
        let b = class(&cx, 3, "B");
        let ab = class(&cx, 4, "AB");
        let ba = class(&cx, 5, "BA");
        space.define_class(&cx, object.clone(), vec![]).unwrap();
        space
            .define_class(&cx, a.clone(), vec![object.clone()])
            .unwrap();
        space.define_class(&cx, b.clone(), vec![object]).unwrap();
        space
            .define_class(&cx, ab.clone(), vec![a.clone(), b.clone()])
            .unwrap();
        space.define_class(&cx, ba.clone(), vec![b, a]).unwrap();
        assert_eq!(
            space.define_class(&cx, class(&cx, 6, "Impossible"), vec![ab, ba]),
            Err(ClassError::InconsistentMro)
        );

        let cyclic = class(&cx, 7, "Cyclic");
        assert_eq!(
            space.define_class(&cx, cyclic.clone(), vec![cyclic]),
            Err(ClassError::UnknownBase(ClassId(7)))
        );
    }

    #[test]
    fn unreachable_python_class_reclaims_ephemeron_owned_mro() {
        let cx = sim_kernel::testing::bare_cx();
        let mut space = PythonObjectSpace::default();
        let class = class(&cx, 20, "Temporary");
        space.define_class(&cx, class, vec![]).unwrap();
        space.release_class(ClassId(20)).unwrap();
        let receipt = space
            .collect_classes(CollectionLimits {
                objects: 16,
                edges: 16,
                stack: 16,
                work: 128,
                clears: 16,
                finalizers: 0,
            })
            .unwrap();
        assert_eq!(receipt.cleared_ephemerons.len(), 1);
        assert_eq!(space.cache.managed_len(), 1);
    }
}
