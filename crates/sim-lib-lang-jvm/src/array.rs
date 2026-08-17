//! Fixed-length Java array payloads composed with the shared managed graph.

use std::sync::Arc;

use sim_lib_mutation::{ArenaError, EdgeId, ManagedHandle};

use crate::{
    FailureCondition, JavaClassMetadata, JavaHierarchyCheck, JavaThrowable, JvmEdge, JvmGraphError,
    JvmHeap, JvmReference, JvmRole, JvmValue,
};

/// The JVMS structural ceiling for an array descriptor's dimensions.
pub const MAX_ARRAY_DIMENSIONS: usize = 255;

/// Primitive array identity, distinct from JVM computational categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayPrimitive {
    /// `boolean[]`.
    Boolean,
    /// `byte[]`.
    Byte,
    /// `char[]`.
    Char,
    /// `short[]`.
    Short,
    /// `int[]`.
    Int,
    /// `float[]`.
    Float,
    /// `long[]`.
    Long,
    /// `double[]`.
    Double,
}

/// Component contract fixed at allocation.
#[derive(Clone, Debug)]
pub enum ArrayComponent {
    /// A primitive array, with no managed element edges.
    Primitive(ArrayPrimitive),
    /// A covariant reference array with its derived component identity.
    Reference(Arc<JavaClassMetadata>),
}

/// Failure while allocating array storage.
#[derive(Debug)]
pub enum ArrayAllocationError {
    /// A Java guest-visible allocation failure.
    Java(Box<JavaThrowable>),
    /// The requested rank exceeds the declared JVMS ceiling.
    DimensionLimit {
        /// Requested rank.
        requested: usize,
        /// Greatest admitted rank.
        limit: usize,
    },
    /// Shared managed allocation failed.
    Managed(ArenaError),
    /// Shared graph mutation failed.
    Graph(JvmGraphError),
}

/// Failure while loading or storing an array element.
#[derive(Debug)]
pub enum ArrayOperationError {
    /// A Java guest-visible check failure.
    Java(Box<JavaThrowable>),
    /// The supplied value category does not match the array component.
    Category,
    /// Shared graph mutation failed without changing the payload.
    Graph(JvmGraphError),
}

/// A dense, fixed-length Java array payload and its managed graph identity.
#[derive(Debug)]
pub struct JavaArray {
    handle: ManagedHandle,
    component: ArrayComponent,
    elements: Vec<JvmValue>,
    edges: Vec<Option<EdgeId>>,
}

impl JavaArray {
    /// Allocates a default-initialized array. Negative lengths raise
    /// `NegativeArraySizeException` through the caller-provided envelope.
    pub fn allocate<F>(
        heap: &mut JvmHeap,
        component: ArrayComponent,
        length: i32,
        mut throwable: F,
    ) -> Result<Self, ArrayAllocationError>
    where
        F: FnMut(FailureCondition) -> JavaThrowable,
    {
        if length < 0 {
            return Err(ArrayAllocationError::Java(Box::new(throwable(
                FailureCondition::NegativeArraySize,
            ))));
        }
        let handle = heap
            .allocate(JvmRole::Array)
            .map_err(ArrayAllocationError::Managed)?;
        let length = length as usize;
        let default = match component {
            ArrayComponent::Primitive(
                ArrayPrimitive::Boolean
                | ArrayPrimitive::Byte
                | ArrayPrimitive::Char
                | ArrayPrimitive::Short
                | ArrayPrimitive::Int,
            ) => JvmValue::Int(0),
            ArrayComponent::Primitive(ArrayPrimitive::Float) => JvmValue::Float(0),
            ArrayComponent::Primitive(ArrayPrimitive::Long) => JvmValue::Long(0),
            ArrayComponent::Primitive(ArrayPrimitive::Double) => JvmValue::Double(0),
            ArrayComponent::Reference(_) => JvmValue::Reference(JvmReference::NULL),
        };
        Ok(Self {
            handle,
            component,
            elements: vec![default; length],
            edges: vec![None; length],
        })
    }

    /// Managed identity of this array.
    pub const fn handle(&self) -> ManagedHandle {
        self.handle
    }

    /// Fixed array length.
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Whether the fixed array is empty.
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Returns the length of a nullable array reference. Null raises
    /// `NullPointerException` through the caller-provided envelope.
    pub fn length_of<F>(
        array: Option<&Self>,
        mut throwable: F,
    ) -> Result<usize, ArrayOperationError>
    where
        F: FnMut(FailureCondition) -> JavaThrowable,
    {
        array.map(Self::len).ok_or_else(|| {
            ArrayOperationError::Java(Box::new(throwable(FailureCondition::NullDereference)))
        })
    }

    /// Loads an element after the Java bounds check.
    pub fn load<F>(&self, index: i32, mut throwable: F) -> Result<&JvmValue, ArrayOperationError>
    where
        F: FnMut(FailureCondition) -> JavaThrowable,
    {
        self.index(index, &mut throwable)
            .map(|index| &self.elements[index])
    }

    /// Stores a primitive value after category and bounds checks.
    pub fn store_primitive<F>(
        &mut self,
        index: i32,
        value: JvmValue,
        mut throwable: F,
    ) -> Result<(), ArrayOperationError>
    where
        F: FnMut(FailureCondition) -> JavaThrowable,
    {
        let index = self.index(index, &mut throwable)?;
        let matches = matches!(
            (&self.component, &value),
            (
                ArrayComponent::Primitive(
                    ArrayPrimitive::Boolean
                        | ArrayPrimitive::Byte
                        | ArrayPrimitive::Char
                        | ArrayPrimitive::Short
                        | ArrayPrimitive::Int
                ),
                JvmValue::Int(_)
            ) | (
                ArrayComponent::Primitive(ArrayPrimitive::Float),
                JvmValue::Float(_)
            ) | (
                ArrayComponent::Primitive(ArrayPrimitive::Long),
                JvmValue::Long(_)
            ) | (
                ArrayComponent::Primitive(ArrayPrimitive::Double),
                JvmValue::Double(_)
            )
        );
        if !matches {
            return Err(ArrayOperationError::Category);
        }
        self.elements[index] = value;
        Ok(())
    }

    /// Stores a nullable reference, enforcing Java covariance and synchronizing
    /// the element's retaining managed edge before changing the payload.
    pub fn store_reference<F>(
        &mut self,
        heap: &mut JvmHeap,
        index: i32,
        value: JvmReference,
        value_class: Option<&JavaClassMetadata>,
        hierarchy_limit: usize,
        mut throwable: F,
    ) -> Result<(), ArrayOperationError>
    where
        F: FnMut(FailureCondition) -> JavaThrowable,
    {
        let index = self.index(index, &mut throwable)?;
        let ArrayComponent::Reference(expected) = &self.component else {
            return Err(ArrayOperationError::Category);
        };
        if value.handle().is_some() {
            let compatible = value_class.is_some_and(|actual| {
                actual.is_assignable_to_binary_name(
                    expected.resolution().binary_name(),
                    hierarchy_limit,
                ) == JavaHierarchyCheck::Match
            });
            if !compatible {
                return Err(ArrayOperationError::Java(Box::new(throwable(
                    FailureCondition::ArrayStore,
                ))));
            }
        }
        let previous = match self.elements[index] {
            JvmValue::Reference(reference) => reference,
            _ => unreachable!(),
        };
        match (self.edges[index], previous.handle(), value.handle()) {
            (None, None, Some(target)) => {
                self.edges[index] = Some(
                    heap.strong(self.handle, JvmEdge::Element, target)
                        .map_err(ArrayOperationError::Graph)?,
                )
            }
            (Some(edge), Some(old), Some(target)) => heap
                .replace_strong(self.handle, edge, old, target)
                .map_err(ArrayOperationError::Graph)?,
            (Some(edge), Some(old), None) => {
                heap.remove_strong(self.handle, edge, old)
                    .map_err(ArrayOperationError::Graph)?;
                self.edges[index] = None;
            }
            (None, None, None) => {}
            _ => unreachable!("array payload and managed edges remain synchronized"),
        }
        self.elements[index] = JvmValue::Reference(value);
        Ok(())
    }

    fn index<F>(&self, index: i32, throwable: &mut F) -> Result<usize, ArrayOperationError>
    where
        F: FnMut(FailureCondition) -> JavaThrowable,
    {
        usize::try_from(index)
            .ok()
            .filter(|index| *index < self.len())
            .ok_or_else(|| {
                ArrayOperationError::Java(Box::new(throwable(
                    FailureCondition::ArrayIndexOutOfBounds,
                )))
            })
    }
}

/// Preorder ownership of every payload created by one multidimensional allocation.
#[derive(Debug)]
pub struct JavaArrayTree {
    arrays: Vec<JavaArray>,
}

impl JavaArrayTree {
    /// Constructs all requested dimensions. Rank zero and rank above 255 are
    /// structural refusals; negative lengths use Java throwable completion.
    pub fn allocate<F>(
        heap: &mut JvmHeap,
        leaf: ArrayComponent,
        dimensions: &[i32],
        mut throwable: F,
    ) -> Result<Self, ArrayAllocationError>
    where
        F: FnMut(FailureCondition) -> JavaThrowable,
    {
        if dimensions.is_empty() || dimensions.len() >= MAX_ARRAY_DIMENSIONS {
            return Err(ArrayAllocationError::DimensionLimit {
                requested: dimensions.len(),
                limit: MAX_ARRAY_DIMENSIONS - 1,
            });
        }
        let mut arrays = Vec::new();
        allocate_level(heap, &leaf, dimensions, 0, &mut arrays, &mut throwable)?;
        Ok(Self { arrays })
    }

    /// Root array payload.
    pub fn root(&self) -> &JavaArray {
        &self.arrays[0]
    }

    /// Every allocated payload in preorder.
    pub fn arrays(&self) -> &[JavaArray] {
        &self.arrays
    }
}

fn allocate_level<F>(
    heap: &mut JvmHeap,
    leaf: &ArrayComponent,
    dimensions: &[i32],
    depth: usize,
    arrays: &mut Vec<JavaArray>,
    throwable: &mut F,
) -> Result<ManagedHandle, ArrayAllocationError>
where
    F: FnMut(FailureCondition) -> JavaThrowable,
{
    let component = if depth + 1 == dimensions.len() {
        leaf.clone()
    } else {
        // Nested payload identity is managed here; covariance metadata remains on leaf arrays.
        match leaf {
            ArrayComponent::Reference(class) => ArrayComponent::Reference(class.clone()),
            ArrayComponent::Primitive(kind) => ArrayComponent::Primitive(*kind),
        }
    };
    let array = JavaArray::allocate(heap, component, dimensions[depth], &mut *throwable)?;
    let handle = array.handle();
    let own_index = arrays.len();
    arrays.push(array);
    if depth + 1 < dimensions.len() {
        for index in 0..dimensions[depth] {
            let child = allocate_level(heap, leaf, dimensions, depth + 1, arrays, &mut *throwable)?;
            let edge = heap
                .strong(handle, JvmEdge::Element, child)
                .map_err(ArrayAllocationError::Graph)?;
            arrays[own_index].elements[index as usize] =
                JvmValue::Reference(JvmReference::managed(child));
            arrays[own_index].edges[index as usize] = Some(edge);
        }
    }
    Ok(handle)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_kernel::{CodecId, Cx, DefaultFactory, NoopEvalPolicy, Origin, SourceId, Span, Symbol};
    use sim_lib_control::Raised;
    use sim_lib_gc_tracing::CollectionLimits;

    use super::*;

    fn limits() -> CollectionLimits {
        CollectionLimits {
            objects: 1024,
            edges: 4096,
            stack: 1024,
            work: 16384,
            clears: 1024,
            finalizers: 0,
        }
    }

    fn throwable(condition: FailureCondition) -> JavaThrowable {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let class = condition.java_class().unwrap();
        let raised = Raised::new(
            cx.factory().symbol(Symbol::new(class)).unwrap(),
            cx.factory().string(class.into()).unwrap(),
            Origin {
                codec: CodecId(0),
                source: SourceId("array-test".into()),
                span: Span { start: 0, end: 1 },
                trivia: vec![],
            },
            Symbol::new("java/jvm"),
        )
        .unwrap();
        JavaThrowable::new(condition, raised).unwrap()
    }

    fn class(name: &str, parents: &[&str]) -> Arc<JavaClassMetadata> {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        Arc::new(JavaClassMetadata::test_identity(&cx, name, parents))
    }

    #[test]
    fn array_store_checks_covariance_and_reports_java_class() {
        let animal = class("Animal", &["java.lang.Object"]);
        let dog = class("Dog", &["Animal"]);
        let stone = class("Stone", &["java.lang.Object"]);
        let mut heap = JvmHeap::new(16, limits()).unwrap();
        let dog_object = heap.allocate(JvmRole::Object).unwrap();
        let stone_object = heap.allocate(JvmRole::Object).unwrap();
        let mut array =
            JavaArray::allocate(&mut heap, ArrayComponent::Reference(animal), 1, throwable)
                .unwrap();

        array
            .store_reference(
                &mut heap,
                0,
                JvmReference::managed(dog_object),
                Some(&dog),
                8,
                throwable,
            )
            .unwrap();
        let error = array
            .store_reference(
                &mut heap,
                0,
                JvmReference::managed(stone_object),
                Some(&stone),
                8,
                throwable,
            )
            .unwrap_err();
        let ArrayOperationError::Java(error) = error else {
            panic!("expected Java throwable")
        };
        assert_eq!(error.condition(), FailureCondition::ArrayStore);
        assert_eq!(
            error
                .raised()
                .class_ref()
                .object()
                .display(&mut Cx::new(
                    Arc::new(NoopEvalPolicy),
                    Arc::new(DefaultFactory)
                ))
                .unwrap(),
            "java/lang/ArrayStoreException"
        );
    }

    #[test]
    fn rank_limit_and_safepoint_retention_are_explicit() {
        let mut heap = JvmHeap::new(1024, limits()).unwrap();
        let error = JavaArrayTree::allocate(
            &mut heap,
            ArrayComponent::Primitive(ArrayPrimitive::Int),
            &vec![0; 255],
            throwable,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ArrayAllocationError::DimensionLimit {
                requested: 255,
                limit: 254
            }
        ));
        let null_error = JavaArray::length_of(None, throwable).unwrap_err();
        assert!(
            matches!(null_error, ArrayOperationError::Java(error) if error.condition() == FailureCondition::NullDereference)
        );

        let element = heap.allocate(JvmRole::Object).unwrap();
        let component = class("java.lang.Object", &[]);
        let mut array = JavaArray::allocate(
            &mut heap,
            ArrayComponent::Reference(component.clone()),
            1,
            throwable,
        )
        .unwrap();
        array
            .store_reference(
                &mut heap,
                0,
                JvmReference::managed(element),
                Some(&component),
                8,
                throwable,
            )
            .unwrap();
        let root = heap.root(array.handle()).unwrap();
        assert!(
            heap.collect().unwrap().swept.is_empty(),
            "a safepoint trace retains array elements through managed edges"
        );
        heap.release_root(root).unwrap();
        let swept = heap.collect().unwrap().swept;
        assert!(swept.contains(&array.handle().id()) && swept.contains(&element.id()));
    }
}
