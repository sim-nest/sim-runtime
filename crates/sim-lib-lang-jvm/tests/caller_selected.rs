use std::sync::Arc;

use sim_kernel::{CapabilityName, Cx, DefaultFactory, NoopEvalPolicy};
use sim_lib_lang_jvm::{
    JvmExecutionOutcome, JvmExecutionRequest, JvmSurface, class_load_capability,
    jvm_invoke_capability,
};

fn request(bytes: Vec<u8>, member: &str, arguments: Vec<i32>) -> JvmExecutionRequest {
    JvmExecutionRequest {
        classfile: bytes,
        class: "StaticInt".into(),
        member: member.into(),
        descriptor: "(II)I".into(),
        arguments,
    }
}

#[test]
fn caller_selected_bytes_member_descriptor_and_arguments_drive_execution() {
    let mut cx = Cx::new(
        Arc::new(NoopEvalPolicy),
        Arc::new(DefaultFactory),
        sim_kernel::HandleSeed::new(0x4a56_4d04),
    );
    cx.grant(class_load_capability());
    cx.grant(jvm_invoke_capability());
    let bytes = include_bytes!("../fixtures/javac/StaticInt.class").to_vec();
    let outcome =
        JvmSurface::new(1 << 20).execute_i32(&mut cx, request(bytes, "wholePipeline", vec![5, 6]));
    assert!(matches!(outcome, JvmExecutionOutcome::Value(22)));
}

#[test]
fn public_route_distinguishes_refusal_cases() {
    let bytes = include_bytes!("../fixtures/javac/StaticInt.class").to_vec();
    let mut denied = Cx::new(
        Arc::new(NoopEvalPolicy),
        Arc::new(DefaultFactory),
        sim_kernel::HandleSeed::new(0x4a56_4d05),
    );
    assert!(matches!(
        JvmSurface::new(1 << 20).execute_i32(
            &mut denied,
            request(bytes.clone(), "wholePipeline", vec![3, 4])
        ),
        JvmExecutionOutcome::Refusal(_)
    ));

    let mut cx = Cx::new(
        Arc::new(NoopEvalPolicy),
        Arc::new(DefaultFactory),
        sim_kernel::HandleSeed::new(0x4a56_4d06),
    );
    for capability in [class_load_capability(), jvm_invoke_capability()] {
        cx.grant(capability);
    }
    for bad in [
        request(vec![0; 65], "wholePipeline", vec![3, 4]),
        request(bytes.clone(), "missing", vec![3, 4]),
        JvmExecutionRequest {
            descriptor: "(J)J".into(),
            arguments: vec![3],
            ..request(bytes, "nonIntParameter", vec![])
        },
    ] {
        assert!(matches!(
            JvmSurface::new(64).execute_i32(&mut cx, bad),
            JvmExecutionOutcome::Refusal(_)
        ));
    }
    let _capability_name: CapabilityName = jvm_invoke_capability();
}
