// conformance: the JVM product specimen runs through the loadable public surface.

use std::sync::Arc;

use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
use sim_lib_lang_jvm::{class_load_capability, jvm_invoke_capability, run_product_specimen};

#[test]
fn published_specimen_runs_all_product_cases() {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    cx.grant(class_load_capability());
    cx.grant(jvm_invoke_capability());
    let report = run_product_specimen(&mut cx).unwrap();
    assert_eq!(report.static_result, 14);
    assert!(report.object_allocated);
    assert_eq!(report.array_result, 17);
    assert_eq!(
        report.exception_class,
        "java/lang/NegativeArraySizeException"
    );
    assert_eq!(report.concat_result, "SIM JVM");
}
