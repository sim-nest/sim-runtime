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
        Ok(self
            .heap
            .allocate(JavascriptManagedObject::new(JavascriptManagedKind::Object))?)
    }
    /// Allocate a closure/function object and connect it to its environment.
    pub fn function(
        &mut self,
        function: JavascriptFunction,
        lexical_this: Option<JavascriptValue>,
    ) -> Result<ManagedHandle, JavascriptObjectError> {
        let h = self.heap.allocate(JavascriptManagedObject::new(
            JavascriptManagedKind::Function,
        ))?;
        for capture in function.captures() {
            self.heap.connect(h, capture.managed())?;
        }
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
        Ok(if f.policy.kind == JavascriptFunctionKind::Arrow {
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
        body: impl FnOnce(
            &FunctionPlan,
            &[CapturedBinding],
            JavascriptThis,
            &[JavascriptValue],
        ) -> Result<T, E>,
    ) -> Result<T, E>
    where
        E: From<JavascriptObjectError>,
    {
        let metadata = self
            .functions
            .get(&function.id())
            .ok_or(JavascriptObjectError::NotConstructor)?;
        let this = self.call_this(function, receiver)?;
        body(metadata.plan(), metadata.captures(), this, arguments)
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
        if !f.policy.constructable || f.policy.kind == JavascriptFunctionKind::Arrow {
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
