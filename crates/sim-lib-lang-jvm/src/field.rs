//! Descriptor-driven instance fields and class-owned static storage.

use std::sync::Arc;

use sim_lib_mutation::{EdgeId, ManagedHandle};

use crate::{
    JavaClassMetadata, JavaMember, JavaMemberKind, JvmEdge, JvmGraphError, JvmHeap, JvmReference,
    JvmRole, JvmValue,
};

/// Stable index of a field in its instance or static layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldId(usize);

/// Caller facts needed to enforce JVM member access without ambient authority.
#[derive(Clone, Copy, Debug)]
pub struct FieldAccess<'a> {
    /// Binary name of the accessing class.
    pub caller: &'a str,
    /// Whether the caller is a subclass of the declaring class.
    pub caller_is_subclass: bool,
}

/// Context in which a field write occurs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteContext<'a> {
    /// An ordinary bytecode write.
    Ordinary,
    /// A write performed by the named class's instance initializer.
    InstanceInitializer(&'a str),
    /// A write performed by the named class's class initializer.
    ClassInitializer(&'a str),
}

/// Lifecycle of the forward class-initialization seam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitializationState {
    /// No active use has requested initialization.
    Uninitialized,
    /// The initializer is currently running.
    Initializing,
    /// Initialization completed successfully.
    Initialized,
    /// Initialization failed; later active uses are refused.
    Failed,
}

/// Checked field/storage failure.
#[derive(Debug)]
pub enum FieldError {
    /// The selected field does not exist in this layout.
    UnknownField,
    /// An instance operation received the JVM null reference.
    NullReceiver,
    /// The caller cannot access the selected declaration.
    IllegalAccess,
    /// A final field write did not occur in its declaring initializer.
    FinalWrite,
    /// The value's computational category does not match the field descriptor.
    TypeMismatch,
    /// Class initialization failed or was recursively requested.
    InitializationFailed,
    /// Managed graph mutation failed.
    Graph(JvmGraphError),
}

impl From<JvmGraphError> for FieldError {
    fn from(value: JvmGraphError) -> Self {
        Self::Graph(value)
    }
}

#[derive(Clone, Debug)]
struct FieldSlot {
    declaration: JavaMember,
    value: JvmValue,
    edge: Option<(EdgeId, ManagedHandle)>,
}

/// Immutable descriptor-derived division of instance and static declarations.
#[derive(Clone, Debug)]
pub struct FieldLayout {
    declaring_class: String,
    instance: Vec<JavaMember>,
    statics: Vec<JavaMember>,
}

impl FieldLayout {
    /// Builds the exact declared layout in classfile order.
    pub fn declared(metadata: &JavaClassMetadata) -> Self {
        let mut instance = Vec::new();
        let mut statics = Vec::new();
        for member in metadata
            .members()
            .iter()
            .filter(|member| member.kind() == JavaMemberKind::Field)
        {
            if member.is_static() {
                statics.push(member.clone());
            } else {
                instance.push(member.clone());
            }
        }
        Self {
            declaring_class: metadata.resolution().binary_name().to_owned(),
            instance,
            statics,
        }
    }

    #[cfg(test)]
    fn test(declaring_class: &str, fields: Vec<JavaMember>) -> Self {
        let (statics, instance): (Vec<_>, Vec<_>) =
            fields.into_iter().partition(JavaMember::is_static);
        Self {
            declaring_class: declaring_class.into(),
            instance,
            statics,
        }
    }

    /// Locates an instance field by exact name and descriptor.
    pub fn instance_field(&self, name: &str, descriptor: &str) -> Option<FieldId> {
        find(&self.instance, name, descriptor)
    }

    /// Locates a static field by exact name and descriptor.
    pub fn static_field(&self, name: &str, descriptor: &str) -> Option<FieldId> {
        find(&self.statics, name, descriptor)
    }
}

/// Values for one instance plus the class-owned static region for its layout.
pub struct FieldStorage {
    layout: Arc<FieldLayout>,
    statics_owner: ManagedHandle,
    statics: Vec<FieldSlot>,
    initialization: InitializationState,
}

impl FieldStorage {
    /// Allocates class-owned static storage and attaches it to the class mirror.
    pub fn prepare(
        heap: &mut JvmHeap,
        class: ManagedHandle,
        layout: Arc<FieldLayout>,
    ) -> Result<Self, FieldError> {
        let statics_owner = heap
            .allocate(JvmRole::Statics)
            .map_err(JvmGraphError::from)?;
        heap.strong(class, JvmEdge::StaticStorage, statics_owner)?;
        let statics = slots(&layout.statics)?;
        Ok(Self {
            layout,
            statics_owner,
            statics,
            initialization: InitializationState::Uninitialized,
        })
    }

    /// Allocates an object, attaches its class, and default-initializes its slots.
    pub fn allocate_instance(
        &self,
        heap: &mut JvmHeap,
        class: ManagedHandle,
    ) -> Result<JavaObject, FieldError> {
        let handle = heap
            .allocate(JvmRole::Object)
            .map_err(JvmGraphError::from)?;
        heap.strong(handle, JvmEdge::Class, class)?;
        Ok(JavaObject {
            handle,
            layout: self.layout.clone(),
            fields: slots(&self.layout.instance)?,
        })
    }

    /// Current initialization state.
    pub const fn initialization_state(&self) -> InitializationState {
        self.initialization
    }

    /// Reads a static field, triggering the supplied initializer at most once.
    pub fn get_static(
        &mut self,
        field: FieldId,
        access: FieldAccess<'_>,
        initialize: impl FnOnce(&mut Self) -> Result<(), FieldError>,
    ) -> Result<JvmValue, FieldError> {
        let slot = self.statics.get(field.0).ok_or(FieldError::UnknownField)?;
        check_access(&self.layout.declaring_class, &slot.declaration, access)?;
        self.ensure_initialized(initialize)?;
        Ok(self.statics[field.0].value.clone())
    }

    /// Writes a static field and records reference replacement in the graph.
    pub fn put_static(
        &mut self,
        heap: &mut JvmHeap,
        field: FieldId,
        value: JvmValue,
        access: FieldAccess<'_>,
        context: WriteContext<'_>,
        initialize: impl FnOnce(&mut Self) -> Result<(), FieldError>,
    ) -> Result<(), FieldError> {
        {
            let slot = self.statics.get(field.0).ok_or(FieldError::UnknownField)?;
            check_access(&self.layout.declaring_class, &slot.declaration, access)?;
            check_final(
                &self.layout.declaring_class,
                &slot.declaration,
                context,
                true,
            )?;
            if !value_matches(slot.declaration.descriptor(), &value) {
                return Err(FieldError::TypeMismatch);
            }
        }
        self.ensure_initialized(initialize)?;
        let slot = self
            .statics
            .get_mut(field.0)
            .ok_or(FieldError::UnknownField)?;
        write_slot(heap, self.statics_owner, JvmEdge::StaticValue, slot, value)
    }

    /// Writes a static from the declaring class initializer while initialization is active.
    pub fn put_static_from_initializer(
        &mut self,
        heap: &mut JvmHeap,
        field: FieldId,
        value: JvmValue,
        access: FieldAccess<'_>,
    ) -> Result<(), FieldError> {
        if self.initialization != InitializationState::Initializing {
            return Err(FieldError::InitializationFailed);
        }
        let slot = self
            .statics
            .get_mut(field.0)
            .ok_or(FieldError::UnknownField)?;
        check_access(&self.layout.declaring_class, &slot.declaration, access)?;
        check_final(
            &self.layout.declaring_class,
            &slot.declaration,
            WriteContext::ClassInitializer(&self.layout.declaring_class),
            true,
        )?;
        write_slot(heap, self.statics_owner, JvmEdge::StaticValue, slot, value)
    }

    fn ensure_initialized(
        &mut self,
        initialize: impl FnOnce(&mut Self) -> Result<(), FieldError>,
    ) -> Result<(), FieldError> {
        match self.initialization {
            InitializationState::Initialized => return Ok(()),
            InitializationState::Initializing | InitializationState::Failed => {
                return Err(FieldError::InitializationFailed);
            }
            InitializationState::Uninitialized => {}
        }
        self.initialization = InitializationState::Initializing;
        match initialize(self) {
            Ok(()) => {
                self.initialization = InitializationState::Initialized;
                Ok(())
            }
            Err(error) => {
                self.initialization = InitializationState::Failed;
                Err(error)
            }
        }
    }
}

/// One managed Java object with descriptor-laid-out instance fields.
pub struct JavaObject {
    handle: ManagedHandle,
    layout: Arc<FieldLayout>,
    fields: Vec<FieldSlot>,
}

impl JavaObject {
    /// Managed identity used as the owner of every reference-field edge.
    pub const fn handle(&self) -> ManagedHandle {
        self.handle
    }

    /// Reads a checked instance field.
    pub fn get(
        &self,
        receiver: JvmReference,
        field: FieldId,
        access: FieldAccess<'_>,
    ) -> Result<JvmValue, FieldError> {
        check_receiver(receiver, self.handle)?;
        let slot = self.fields.get(field.0).ok_or(FieldError::UnknownField)?;
        check_access(&self.layout.declaring_class, &slot.declaration, access)?;
        Ok(slot.value.clone())
    }

    /// Writes a checked instance field and records reference replacement.
    pub fn put(
        &mut self,
        heap: &mut JvmHeap,
        receiver: JvmReference,
        field: FieldId,
        value: JvmValue,
        access: FieldAccess<'_>,
        context: WriteContext<'_>,
    ) -> Result<(), FieldError> {
        check_receiver(receiver, self.handle)?;
        let slot = self
            .fields
            .get_mut(field.0)
            .ok_or(FieldError::UnknownField)?;
        check_access(&self.layout.declaring_class, &slot.declaration, access)?;
        check_final(
            &self.layout.declaring_class,
            &slot.declaration,
            context,
            false,
        )?;
        write_slot(heap, self.handle, JvmEdge::Field, slot, value)
    }
}

fn find(fields: &[JavaMember], name: &str, descriptor: &str) -> Option<FieldId> {
    fields
        .iter()
        .position(|field| field.name() == name && field.descriptor() == descriptor)
        .map(FieldId)
}

fn slots(declarations: &[JavaMember]) -> Result<Vec<FieldSlot>, FieldError> {
    declarations
        .iter()
        .map(|declaration| {
            Ok(FieldSlot {
                value: default_value(declaration.descriptor())?,
                declaration: declaration.clone(),
                edge: None,
            })
        })
        .collect()
}

fn default_value(descriptor: &str) -> Result<JvmValue, FieldError> {
    Ok(match descriptor.as_bytes().first() {
        Some(b'B' | b'C' | b'I' | b'S' | b'Z') => JvmValue::Int(0),
        Some(b'F') => JvmValue::Float(0),
        Some(b'J') => JvmValue::Long(0),
        Some(b'D') => JvmValue::Double(0),
        Some(b'L' | b'[') => JvmValue::Reference(JvmReference::NULL),
        _ => return Err(FieldError::TypeMismatch),
    })
}

fn value_matches(descriptor: &str, value: &JvmValue) -> bool {
    matches!(
        (descriptor.as_bytes().first(), value),
        (Some(b'B' | b'C' | b'I' | b'S' | b'Z'), JvmValue::Int(_))
            | (Some(b'F'), JvmValue::Float(_))
            | (Some(b'J'), JvmValue::Long(_))
            | (Some(b'D'), JvmValue::Double(_))
            | (Some(b'L' | b'['), JvmValue::Reference(_))
    )
}

fn check_receiver(receiver: JvmReference, expected: ManagedHandle) -> Result<(), FieldError> {
    match receiver.handle() {
        None => Err(FieldError::NullReceiver),
        Some(actual) if actual == expected => Ok(()),
        Some(_) => Err(FieldError::TypeMismatch),
    }
}

fn check_access(
    declaring: &str,
    field: &JavaMember,
    access: FieldAccess<'_>,
) -> Result<(), FieldError> {
    let flags = field.access_flags();
    let same_class = access.caller == declaring;
    let same_package = package(access.caller) == package(declaring);
    let admitted = flags & 0x0001 != 0
        || (flags & 0x0002 != 0 && same_class)
        || (flags & 0x0004 != 0 && (same_package || access.caller_is_subclass))
        || (flags & 0x0007 == 0 && same_package);
    admitted.then_some(()).ok_or(FieldError::IllegalAccess)
}

fn package(binary_name: &str) -> &str {
    binary_name
        .rsplit_once('.')
        .map_or("", |(package, _)| package)
}

fn check_final(
    declaring: &str,
    field: &JavaMember,
    context: WriteContext<'_>,
    static_field: bool,
) -> Result<(), FieldError> {
    if !field.is_final() {
        return Ok(());
    }
    let legal = match (static_field, context) {
        (true, WriteContext::ClassInitializer(owner)) => owner == declaring,
        (false, WriteContext::InstanceInitializer(owner)) => owner == declaring,
        _ => false,
    };
    legal.then_some(()).ok_or(FieldError::FinalWrite)
}

fn write_slot(
    heap: &mut JvmHeap,
    owner: ManagedHandle,
    edge_kind: JvmEdge,
    slot: &mut FieldSlot,
    value: JvmValue,
) -> Result<(), FieldError> {
    if !value_matches(slot.declaration.descriptor(), &value) {
        return Err(FieldError::TypeMismatch);
    }
    let replacement = match &value {
        JvmValue::Reference(reference) => reference.handle(),
        _ => None,
    };
    slot.edge = match (slot.edge, replacement) {
        (Some((edge, expected)), Some(target)) => {
            heap.replace_strong(owner, edge, expected, target)?;
            Some((edge, target))
        }
        (Some((edge, expected)), None) => {
            heap.remove_strong(owner, edge, expected)?;
            None
        }
        (None, Some(target)) => Some((heap.strong(owner, edge_kind, target)?, target)),
        (None, None) => None,
    };
    slot.value = value;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, sync::Arc};

    use sim_lib_gc_tracing::CollectionLimits;

    use super::*;

    fn limits() -> CollectionLimits {
        CollectionLimits {
            objects: 32,
            edges: 64,
            stack: 32,
            work: 256,
            clears: 32,
            finalizers: 0,
        }
    }

    fn access() -> FieldAccess<'static> {
        FieldAccess {
            caller: "example.Owner",
            caller_is_subclass: false,
        }
    }

    #[test]
    fn defaults_final_rules_and_null_receivers_are_exact() {
        let layout = Arc::new(FieldLayout::test(
            "example.Owner",
            vec![
                JavaMember::test_field("answer", "I", 0x0011),
                JavaMember::test_field("peer", "Ljava/lang/Object;", 0x0001),
            ],
        ));
        let mut heap = JvmHeap::new(16, limits()).unwrap();
        let class = heap.allocate(JvmRole::ClassMirror).unwrap();
        let storage = FieldStorage::prepare(&mut heap, class, layout.clone()).unwrap();
        let mut object = storage.allocate_instance(&mut heap, class).unwrap();
        let answer = layout.instance_field("answer", "I").unwrap();
        assert!(matches!(
            object.get(JvmReference::managed(object.handle()), answer, access()),
            Ok(JvmValue::Int(0))
        ));
        assert!(matches!(
            object.put(
                &mut heap,
                JvmReference::managed(object.handle()),
                answer,
                JvmValue::Int(1),
                access(),
                WriteContext::Ordinary,
            ),
            Err(FieldError::FinalWrite)
        ));
        assert!(matches!(
            object.get(JvmReference::NULL, answer, access()),
            Err(FieldError::NullReceiver)
        ));
    }

    #[test]
    fn static_read_initializes_once_and_class_owns_static_lifetime() {
        let layout = Arc::new(FieldLayout::test(
            "example.Owner",
            vec![JavaMember::test_field("value", "I", 0x0009)],
        ));
        let mut heap = JvmHeap::new(16, limits()).unwrap();
        let class = heap.allocate(JvmRole::ClassMirror).unwrap();
        let class_root = heap.root(class).unwrap();
        let mut storage = FieldStorage::prepare(&mut heap, class, layout.clone()).unwrap();
        let value = layout.static_field("value", "I").unwrap();
        let calls = Cell::new(0);
        assert!(matches!(
            storage.get_static(value, access(), |_| {
                calls.set(calls.get() + 1);
                Ok(())
            }),
            Ok(JvmValue::Int(0))
        ));
        storage
            .get_static(value, access(), |_| {
                calls.set(calls.get() + 1);
                Ok(())
            })
            .unwrap();
        assert_eq!(calls.get(), 1);
        assert!(heap.collect().unwrap().swept.is_empty());
        heap.release_root(class_root).unwrap();
        let receipt = heap.collect().unwrap();
        assert_eq!(receipt.swept, vec![class.id(), storage.statics_owner.id()]);
    }

    #[test]
    fn final_static_write_outside_declaring_initializer_is_refused() {
        let layout = Arc::new(FieldLayout::test(
            "example.Owner",
            vec![JavaMember::test_field("constant", "I", 0x0019)],
        ));
        let mut heap = JvmHeap::new(16, limits()).unwrap();
        let class = heap.allocate(JvmRole::ClassMirror).unwrap();
        let mut storage = FieldStorage::prepare(&mut heap, class, layout.clone()).unwrap();
        let constant = layout.static_field("constant", "I").unwrap();
        assert!(matches!(
            storage.put_static(
                &mut heap,
                constant,
                JvmValue::Int(7),
                access(),
                WriteContext::Ordinary,
                |_| Ok(()),
            ),
            Err(FieldError::FinalWrite)
        ));
        assert_eq!(
            storage.initialization_state(),
            InitializationState::Uninitialized
        );
    }

    #[test]
    fn every_reference_replacement_updates_the_managed_edge() {
        let layout = Arc::new(FieldLayout::test(
            "example.Owner",
            vec![JavaMember::test_field("peer", "Ljava/lang/Object;", 0x0001)],
        ));
        let mut heap = JvmHeap::new(16, limits()).unwrap();
        let class = heap.allocate(JvmRole::ClassMirror).unwrap();
        let storage = FieldStorage::prepare(&mut heap, class, layout.clone()).unwrap();
        let mut object = storage.allocate_instance(&mut heap, class).unwrap();
        let object_root = heap.root(object.handle()).unwrap();
        let first = heap.allocate(JvmRole::Object).unwrap();
        let second = heap.allocate(JvmRole::Object).unwrap();
        let peer = layout.instance_field("peer", "Ljava/lang/Object;").unwrap();
        for target in [first, second] {
            object
                .put(
                    &mut heap,
                    JvmReference::managed(object.handle()),
                    peer,
                    JvmValue::Reference(JvmReference::managed(target)),
                    access(),
                    WriteContext::Ordinary,
                )
                .unwrap();
        }
        let receipt = heap.collect().unwrap();
        assert_eq!(receipt.swept, vec![first.id()]);
        object
            .put(
                &mut heap,
                JvmReference::managed(object.handle()),
                peer,
                JvmValue::Reference(JvmReference::NULL),
                access(),
                WriteContext::Ordinary,
            )
            .unwrap();
        assert_eq!(heap.collect().unwrap().swept, vec![second.id()]);
        heap.release_root(object_root).unwrap();
    }
}
