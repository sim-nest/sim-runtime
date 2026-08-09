use crate::{
    DescriptorHook, PYTHON_OBJECT_CONTROL_GAPS, PythonClass, PythonObjectSpace, PythonObjectValue,
};

#[test]
fn public_object_control_contract_is_reachable_and_fail_closed() {
    let mut objects = PythonObjectSpace::default();
    objects.define_class(1, "object", vec![]).unwrap();
    objects.define_class(2, "Checked", vec![1]).unwrap();
    objects.instantiate(3, 2).unwrap();
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
    assert_eq!(
        objects.class(2),
        Some(&PythonClass {
            id: 2,
            name: "Checked".into(),
            bases: vec![1],
            mro: vec![2, 1],
        })
    );
    assert_eq!(PYTHON_OBJECT_CONTROL_GAPS.len(), 5);
}
