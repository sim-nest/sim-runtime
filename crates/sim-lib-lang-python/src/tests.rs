use crate::{DescriptorHook, PYTHON_OBJECT_CONTROL_GAPS, PythonObjectSpace, PythonObjectValue};
use sim_kernel::{ClassId, ClassRef, Cx, DefaultFactory, NoopEvalPolicy, Symbol};
use std::sync::Arc;

fn cx() -> Cx {
    Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
}
fn class(cx: &Cx, id: u32, name: &str) -> ClassRef {
    cx.factory()
        .class_stub(ClassId(id), Symbol::qualified("python", name))
        .unwrap()
}

#[test]
fn public_object_control_contract_is_reachable_and_fail_closed() {
    let mut objects = PythonObjectSpace::default();
    let cx = cx();
    let object = class(&cx, 1, "object");
    let checked = class(&cx, 2, "Checked");
    objects.define_class(&cx, object.clone(), vec![]).unwrap();
    objects
        .define_class(&cx, checked.clone(), vec![object.clone()])
        .unwrap();
    objects.instantiate(3, checked.clone()).unwrap();
    objects.define_descriptor(
        2,
        "answer",
        DescriptorHook {
            name: "answer".into(),
            value: PythonObjectValue::Int(42),
        },
        true,
    );
    assert_eq!(objects.get(3, "answer"), Ok(PythonObjectValue::Int(42)));
    let declared = objects.class(ClassId(2)).unwrap();
    assert_eq!(declared.identity, checked);
    assert_eq!(
        declared.descriptor.parents()[0].resolved_class(),
        Some(&object)
    );
    assert_eq!(declared.mro, vec![declared.identity.clone(), object]);
    assert_eq!(PYTHON_OBJECT_CONTROL_GAPS.len(), 5);
}
