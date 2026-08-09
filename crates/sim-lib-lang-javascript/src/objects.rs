//! JavaScript object policy over the shared property and managed-identity organs.

use crate::{JavascriptHeap, JavascriptManagedKind, JavascriptManagedObject, JavascriptValue};
use sim_lib_dispatch::{
    AccessContext, AccessError, AccessorDescriptor, DataDescriptor, DefineError, Descriptor,
    PropertyHook, PropertyStore,
};
use sim_lib_mutation::{ArenaError, ManagedHandle, ManagedId};
use std::collections::{BTreeMap, HashSet};

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

/// Inspectable callable metadata. Executable bodies remain codec-lowered forms;
/// this record owns only binding and construction policy.
#[derive(Clone, Debug)]
pub struct JavascriptFunction {
    /// Function form.
    pub kind: JavascriptFunctionKind,
    /// Captured lexical environment.
    pub environment: ManagedHandle,
    /// Whether `new` is legal.
    pub constructable: bool,
    /// Declared private names for a class constructor.
    pub private_names: Vec<String>,
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

/// Ordinary-object policy composed from `PropertyStore` and `JavascriptHeap`.
pub struct JavascriptObjects {
    heap: JavascriptHeap,
    properties: PropertyStore<ManagedId, JavascriptPropertyKey, JavascriptValue, Accessor>,
    prototypes: BTreeMap<ManagedId, ManagedHandle>,
    functions: BTreeMap<ManagedId, JavascriptFunction>,
    lexical_this: BTreeMap<ManagedId, JavascriptValue>,
}
impl JavascriptObjects {
    /// Create an object graph using the supplied shared heap.
    pub fn new(heap: JavascriptHeap) -> Self {
        Self {
            heap,
            properties: PropertyStore::new(),
            prototypes: BTreeMap::new(),
            functions: BTreeMap::new(),
            lexical_this: BTreeMap::new(),
        }
    }
    /// Allocate an ordinary object or array. Arrays use the same object and
    /// descriptor mechanics; only key ordering differs.
    pub fn ordinary(&mut self) -> Result<ManagedHandle, JavascriptObjectError> {
        Ok(self.heap.allocate(JavascriptManagedObject::default())?)
    }
    /// Allocate a closure/function object and connect it to its environment.
    pub fn function(
        &mut self,
        function: JavascriptFunction,
        lexical_this: Option<JavascriptValue>,
    ) -> Result<ManagedHandle, JavascriptObjectError> {
        let h = self.heap.allocate(JavascriptManagedObject {
            kind: JavascriptManagedKind::Function,
            edges: vec![function.environment.id()],
        })?;
        if let Some(v) = lexical_this {
            self.lexical_this.insert(h.id(), v);
        }
        self.functions.insert(h.id(), function);
        Ok(h)
    }
    /// Select ECMAScript `this` policy without constraining the callable Shape.
    pub fn call_this(
        &self,
        function: ManagedHandle,
        receiver: JavascriptValue,
    ) -> Result<JavascriptThis, JavascriptObjectError> {
        let f = self
            .functions
            .get(&function.id())
            .ok_or(JavascriptObjectError::NotConstructor)?;
        Ok(if f.kind == JavascriptFunctionKind::Arrow {
            JavascriptThis::Lexical(
                self.lexical_this
                    .get(&function.id())
                    .cloned()
                    .unwrap_or(JavascriptValue::Undefined),
            )
        } else {
            JavascriptThis::Dynamic(receiver)
        })
    }
    /// Invoke a codec-lowered body with the captured environment, selected
    /// receiver, and call arguments. The caller supplies evaluation, keeping
    /// executable behavior in the direct evaluator rather than this policy.
    pub fn call<T, E>(
        &self,
        function: ManagedHandle,
        receiver: JavascriptValue,
        arguments: &[JavascriptValue],
        body: impl FnOnce(ManagedHandle, JavascriptThis, &[JavascriptValue]) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<JavascriptObjectError>,
    {
        let metadata = self
            .functions
            .get(&function.id())
            .ok_or(JavascriptObjectError::NotConstructor)?;
        let this = self.call_this(function, receiver)?;
        body(metadata.environment, this, arguments)
    }
    /// Allocate the receiver for `new` and link it to the constructor prototype.
    pub fn construct(
        &mut self,
        function: ManagedHandle,
    ) -> Result<ManagedHandle, JavascriptObjectError> {
        let f = self
            .functions
            .get(&function.id())
            .ok_or(JavascriptObjectError::NotConstructor)?;
        if !f.constructable || f.kind == JavascriptFunctionKind::Arrow {
            return Err(JavascriptObjectError::NotConstructor);
        }
        let instance = self.ordinary()?;
        self.set_prototype(instance, function)?;
        Ok(instance)
    }
    /// Set an ordinary prototype, rejecting cycles.
    pub fn set_prototype(
        &mut self,
        object: ManagedHandle,
        prototype: ManagedHandle,
    ) -> Result<(), JavascriptObjectError> {
        let mut at = Some(prototype);
        let mut seen = HashSet::new();
        while let Some(h) = at {
            if h.id() == object.id() || !seen.insert(h.id()) {
                return Err(JavascriptObjectError::PrototypeCycle);
            }
            at = self.prototypes.get(&h.id()).copied();
        }
        self.heap.connect(object, prototype)?;
        self.prototypes.insert(object.id(), prototype);
        Ok(())
    }
    /// Define an ordinary data property.
    pub fn define_data(
        &mut self,
        object: ManagedHandle,
        key: JavascriptPropertyKey,
        value: JavascriptValue,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<(), JavascriptObjectError> {
        self.properties.define(
            &object.id(),
            key,
            Descriptor::Data(DataDescriptor {
                value,
                writable,
                enumerable,
                configurable,
            }),
        )?;
        Ok(())
    }
    /// Define a bounded accessor property.
    pub fn define_accessor(
        &mut self,
        object: ManagedHandle,
        key: JavascriptPropertyKey,
        get: Option<JavascriptValue>,
        set: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Result<(), JavascriptObjectError> {
        self.properties.define(
            &object.id(),
            key,
            Descriptor::Accessor(AccessorDescriptor {
                get: Some(Accessor { get, set }),
                set: Some(Accessor { get: None, set }),
                enumerable,
                configurable,
            }),
        )?;
        Ok(())
    }
    fn chain(
        &self,
        object: ManagedHandle,
        budget: usize,
    ) -> Result<Vec<ManagedId>, JavascriptObjectError> {
        let mut out = Vec::new();
        let mut at = Some(object);
        let mut seen = HashSet::new();
        while let Some(h) = at {
            if out.len() >= budget {
                return Err(JavascriptObjectError::Access);
            }
            if !seen.insert(h.id()) {
                break;
            }
            out.push(h.id());
            at = self.prototypes.get(&h.id()).copied();
        }
        Ok(out)
    }
    /// Read through the prototype chain with the original receiver.
    pub fn get(
        &self,
        object: ManagedHandle,
        key: &JavascriptPropertyKey,
        budget: usize,
    ) -> Result<Option<JavascriptValue>, JavascriptObjectError> {
        let chain = self.chain(object, budget)?;
        let mut hooks = Hooks::default();
        self.properties
            .get(
                &chain,
                &object.id(),
                key,
                &mut AccessContext::new(budget),
                &mut hooks,
            )
            .map_err(|_| JavascriptObjectError::Access)
    }
    /// Assign through the first descriptor in the prototype chain. Setter
    /// hooks retain the original receiver and share the traversal budget.
    pub fn set(
        &mut self,
        object: ManagedHandle,
        key: &JavascriptPropertyKey,
        value: JavascriptValue,
        budget: usize,
    ) -> Result<bool, JavascriptObjectError> {
        let chain = self.chain(object, budget)?;
        let mut hooks = Hooks::default();
        self.properties
            .set(
                &chain,
                &object.id(),
                key,
                value,
                &mut AccessContext::new(budget),
                &mut hooks,
            )
            .map_err(|_| JavascriptObjectError::Access)
    }
    /// Delete an own property, respecting configurability.
    pub fn delete(
        &mut self,
        object: ManagedHandle,
        key: &JavascriptPropertyKey,
    ) -> Result<bool, JavascriptObjectError> {
        Ok(self.properties.delete(&object.id(), key)?)
    }
    /// ECMAScript ordinary enumeration order: array-index strings ascending,
    /// then other strings in definition order, then symbols. Private names are hidden.
    pub fn enumerable_keys(&self, object: ManagedHandle) -> Vec<JavascriptPropertyKey> {
        let keys = self.properties.own_keys(&object.id(), true);
        let mut indices = Vec::new();
        let mut strings = Vec::new();
        let mut symbols = Vec::new();
        for key in keys {
            match &key {
                JavascriptPropertyKey::String(s) => match s.parse::<u32>() {
                    Ok(n) if n != u32::MAX && n.to_string() == *s => indices.push((n, key)),
                    _ => strings.push(key),
                },
                JavascriptPropertyKey::Symbol(_) => symbols.push(key),
                JavascriptPropertyKey::Private { .. } => {}
            }
        }
        indices.sort_by_key(|(n, _)| *n);
        indices
            .into_iter()
            .map(|(_, k)| k)
            .chain(strings)
            .chain(symbols)
            .collect()
    }
    /// Validate a declared private name against an instance's constructor brand.
    pub fn private_key(
        &self,
        class: ManagedHandle,
        name: &str,
    ) -> Result<JavascriptPropertyKey, JavascriptObjectError> {
        let f = self
            .functions
            .get(&class.id())
            .ok_or(JavascriptObjectError::PrivateBrand)?;
        if !f.private_names.iter().any(|n| n == name) {
            return Err(JavascriptObjectError::PrivateBrand);
        }
        Ok(JavascriptPropertyKey::Private {
            class: class.id(),
            name: name.into(),
        })
    }
    /// Collect unreachable function/environment/prototype/accessor/array cycles.
    pub fn collect(
        &mut self,
    ) -> Result<Option<sim_lib_gc_tracing::CollectionReceipt>, sim_lib_gc_tracing::CollectionError>
    {
        self.heap.collect()
    }
    /// Number of live managed identities.
    pub fn live_len(&self) -> usize {
        self.heap.live_len()
    }
}

/// Callable browse policy is intentionally neutral: no parameter or return
/// constraints are synthesized from JavaScript or TypeScript syntax.
pub const fn javascript_callable_shape_constraints() -> &'static [&'static str] {
    &[]
}
/// Unsupported ordinary-object boundary.
pub const fn javascript_object_gaps() -> &'static [JavascriptObjectGap] {
    &[
        JavascriptObjectGap::ProxyInvariants,
        JavascriptObjectGap::ExoticObjectInvariants,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_lib_gc_tracing::CollectionLimits;
    fn model() -> JavascriptObjects {
        JavascriptObjects::new(
            JavascriptHeap::standard(
                32,
                CollectionLimits {
                    objects: 32,
                    edges: 64,
                    stack: 32,
                    work: 256,
                    clears: 32,
                    finalizers: 0,
                },
            )
            .unwrap(),
        )
    }
    fn s(v: &str) -> JavascriptPropertyKey {
        JavascriptPropertyKey::String(v.into())
    }
    #[test]
    fn descriptors_prototypes_arrays_and_private_names_are_shared_mechanics() {
        let mut m = model();
        let env = m.ordinary().unwrap();
        let class = m
            .function(
                JavascriptFunction {
                    kind: JavascriptFunctionKind::ClassConstructor,
                    environment: env,
                    constructable: true,
                    private_names: vec!["x".into()],
                },
                None,
            )
            .unwrap();
        m.define_data(
            class,
            s("inherited"),
            JavascriptValue::Number(7.0),
            false,
            true,
            false,
        )
        .unwrap();
        let o = m.construct(class).unwrap();
        assert_eq!(
            m.get(o, &s("inherited"), 8).unwrap(),
            Some(JavascriptValue::Number(7.0))
        );
        m.define_data(o, s("10"), JavascriptValue::Null, true, true, true)
            .unwrap();
        m.define_data(o, s("2"), JavascriptValue::Null, true, true, true)
            .unwrap();
        m.define_accessor(
            o,
            s("answer"),
            Some(JavascriptValue::Number(42.0)),
            false,
            true,
            true,
        )
        .unwrap();
        assert_eq!(m.enumerable_keys(o), vec![s("2"), s("10"), s("answer")]);
        assert_eq!(
            m.get(o, &s("answer"), 8).unwrap(),
            Some(JavascriptValue::Number(42.0))
        );
        assert!(m.private_key(class, "x").is_ok());
        assert!(m.private_key(class, "y").is_err());
        assert!(m.delete(o, &s("answer")).unwrap());
    }
    #[test]
    fn functions_arrows_construction_shapes_and_gaps_are_explicit() {
        let mut m = model();
        let env = m.ordinary().unwrap();
        let arrow = m
            .function(
                JavascriptFunction {
                    kind: JavascriptFunctionKind::Arrow,
                    environment: env,
                    constructable: false,
                    private_names: vec![],
                },
                Some(JavascriptValue::String("lexical".into())),
            )
            .unwrap();
        assert_eq!(
            m.call_this(arrow, JavascriptValue::String("dynamic".into()))
                .unwrap(),
            JavascriptThis::Lexical(JavascriptValue::String("lexical".into()))
        );
        let called = m
            .call(
                arrow,
                JavascriptValue::Undefined,
                &[JavascriptValue::Number(42.0)],
                |captured, this, arguments| {
                    Ok::<_, JavascriptObjectError>((captured, this, arguments[0].clone()))
                },
            )
            .unwrap();
        assert_eq!(called.0, env);
        assert_eq!(called.2, JavascriptValue::Number(42.0));
        assert_eq!(
            m.construct(arrow),
            Err(JavascriptObjectError::NotConstructor)
        );
        assert!(javascript_callable_shape_constraints().is_empty());
        assert_eq!(javascript_object_gaps().len(), 2);
    }
    #[test]
    fn mixed_language_cycles_reclaim_without_changing_observed_values() {
        let mut m = model();
        let env = m.ordinary().unwrap();
        let f = m
            .function(
                JavascriptFunction {
                    kind: JavascriptFunctionKind::Function,
                    environment: env,
                    constructable: true,
                    private_names: vec![],
                },
                None,
            )
            .unwrap();
        let array = m.ordinary().unwrap();
        m.set_prototype(array, f).unwrap();
        m.define_accessor(
            array,
            s("stable"),
            Some(JavascriptValue::Number(42.0)),
            false,
            true,
            true,
        )
        .unwrap();
        assert_eq!(
            m.get(array, &s("stable"), 8).unwrap(),
            Some(JavascriptValue::Number(42.0))
        );
        assert_eq!(m.live_len(), 3);
        assert_eq!(m.collect().unwrap().unwrap().swept.len(), 3);
        assert_eq!(m.live_len(), 0);
    }
}
