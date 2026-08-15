//! Projection of checked descriptors onto the kernel [`Class`] protocol.

use std::{any::Any, collections::BTreeSet, sync::Arc};

use sim_kernel::{
    Args, Callable, Class, ClassId, ClassRef, Cx, Error, Object, ObjectCompat, ReadConstructorRef,
    Result, ShapeRef, Symbol, TableRef, Value,
};

use crate::ClassDescriptor;

/// Construction behavior supplied by the language or library declaring a class.
pub trait ClassConstructor: Send + Sync + 'static {
    fn construct(&self, cx: &mut Cx, args: Args) -> Result<Value>;
}

impl<F> ClassConstructor for F
where
    F: Fn(&mut Cx, Args) -> Result<Value> + Send + Sync + 'static,
{
    fn construct(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        self(cx, args)
    }
}

/// Evidence retained for every bounded subclass query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubclassEvidence {
    pub visited_nodes: usize,
    pub performed_work: usize,
}

/// Result of a subclass query, including exact exhaustion evidence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SubclassQuery {
    Answer {
        is_subclass: bool,
        evidence: SubclassEvidence,
    },
    NodeBudgetExhausted {
        limit: usize,
        required: usize,
        evidence: SubclassEvidence,
    },
    WorkBudgetExhausted {
        limit: usize,
        evidence: SubclassEvidence,
    },
}

/// A checked descriptor exposed as an ordinary kernel class object.
pub struct DescriptorClass {
    descriptor: ClassDescriptor,
    constructor: Arc<dyn ClassConstructor>,
    subclass_nodes: usize,
    subclass_work: usize,
}

impl DescriptorClass {
    pub fn new(
        descriptor: ClassDescriptor,
        constructor: impl ClassConstructor,
        subclass_nodes: usize,
        subclass_work: usize,
    ) -> Self {
        Self {
            descriptor,
            constructor: Arc::new(constructor),
            subclass_nodes,
            subclass_work,
        }
    }

    pub fn descriptor(&self) -> &ClassDescriptor {
        &self.descriptor
    }

    /// Runs the kernel subclass relation with caller-visible finite limits.
    pub fn query_subclass(
        &self,
        cx: &mut Cx,
        expected: ClassRef,
        node_limit: usize,
        work_limit: usize,
    ) -> Result<SubclassQuery> {
        let Some(expected) = expected.object().as_class() else {
            return Ok(SubclassQuery::Answer {
                is_subclass: false,
                evidence: SubclassEvidence {
                    visited_nodes: 0,
                    performed_work: 0,
                },
            });
        };
        let mut stack = vec![self.parents(cx)?];
        let mut seen = BTreeSet::from([self.id()]);
        let mut evidence = SubclassEvidence {
            visited_nodes: 1,
            performed_work: 0,
        };
        if self.id() == expected.id() {
            return Ok(SubclassQuery::Answer {
                is_subclass: true,
                evidence,
            });
        }

        while let Some(parents) = stack.last_mut() {
            let Some(parent) = parents.pop() else {
                stack.pop();
                continue;
            };
            if evidence.performed_work == work_limit {
                return Ok(SubclassQuery::WorkBudgetExhausted {
                    limit: work_limit,
                    evidence,
                });
            }
            evidence.performed_work += 1;
            let Some(parent) = parent.object().as_class() else {
                continue;
            };
            if !seen.insert(parent.id()) {
                continue;
            }
            let required = evidence.visited_nodes + 1;
            if required > node_limit {
                return Ok(SubclassQuery::NodeBudgetExhausted {
                    limit: node_limit,
                    required,
                    evidence,
                });
            }
            evidence.visited_nodes = required;
            if parent.id() == expected.id() {
                return Ok(SubclassQuery::Answer {
                    is_subclass: true,
                    evidence,
                });
            }
            stack.push(parent.parents(cx)?);
        }
        Ok(SubclassQuery::Answer {
            is_subclass: false,
            evidence,
        })
    }
}

impl Object for DescriptorClass {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<class {}>", self.descriptor.identity().symbol()))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ObjectCompat for DescriptorClass {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Class"))
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
    fn as_class(&self) -> Option<&dyn Class> {
        Some(self)
    }
}

impl Callable for DescriptorClass {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        self.constructor.construct(cx, args)
    }
    fn browse_args_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(Some(self.descriptor.constructor_shape().clone()))
    }
    fn browse_result_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(Some(self.descriptor.instance_shape().clone()))
    }
}

impl Class for DescriptorClass {
    fn id(&self) -> ClassId {
        self.descriptor.identity().id()
    }
    fn symbol(&self) -> Symbol {
        self.descriptor.identity().symbol().clone()
    }
    fn parents(&self, _cx: &mut Cx) -> Result<Vec<ClassRef>> {
        Ok(self
            .descriptor
            .parents()
            .iter()
            .filter_map(|parent| parent.resolved_class().cloned())
            .collect())
    }
    fn is_subclass_of(&self, cx: &mut Cx, expected: ClassRef) -> Result<bool> {
        match self.query_subclass(cx, expected, self.subclass_nodes, self.subclass_work)? {
            SubclassQuery::Answer { is_subclass, .. } => Ok(is_subclass),
            evidence => Err(Error::Lib(format!(
                "bounded class lineage exhausted: {evidence:?}"
            ))),
        }
    }
    fn constructor_shape(&self, _cx: &mut Cx) -> Result<ShapeRef> {
        Ok(self.descriptor.constructor_shape().clone())
    }
    fn instance_shape(&self, _cx: &mut Cx) -> Result<ShapeRef> {
        Ok(self.descriptor.instance_shape().clone())
    }
    fn read_constructor(&self, _cx: &mut Cx) -> Result<Option<ReadConstructorRef>> {
        Ok(self
            .descriptor
            .read_construction()
            .map(|read| read.constructor.clone()))
    }
    fn members(&self, cx: &mut Cx) -> Result<TableRef> {
        cx.factory().table(
            self.descriptor
                .members()
                .iter()
                .map(|member| (member.name.clone(), member.shape.clone()))
                .collect(),
        )
    }
}
