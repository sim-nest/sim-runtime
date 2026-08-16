//! Published end-to-end specimen for the JVM library surface.

use sim_kernel::{CodecId, Cx, Origin, Result, SourceId, Span, Symbol};
use sim_lib_control::Raised;
use sim_lib_gc_tracing::CollectionLimits;
use sim_text::CodeUnitString;

use crate::{
    ArrayComponent, ArrayPrimitive, FailureCondition, JavaArray, JavaString, JavaThrowable,
    JvmHeap, JvmRole, JvmSurface, JvmValue,
};

/// Results from the five JVM product paths exercised by the public specimen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JvmProductSpecimen {
    /// Result of decoding authorized bytes and invoking a static classfile method.
    pub static_result: i32,
    /// Whether an ordinary object received managed JVM identity.
    pub object_allocated: bool,
    /// Value round-tripped through Java array storage.
    pub array_result: i32,
    /// Guest exception class produced by a checked failing operation.
    pub exception_class: &'static str,
    /// Exact Java UTF-16 concatenation result.
    pub concat_result: String,
}

/// Runs the checked product specimen without an ambient classpath or private API.
pub fn run_product_specimen(cx: &mut Cx) -> Result<JvmProductSpecimen> {
    let surface = JvmSurface::new(1 << 20);
    surface.define(
        cx,
        "StaticInt",
        include_bytes!("../fixtures/javac/StaticInt.class").to_vec(),
    )?;
    let static_result =
        surface.invoke_static_i32(cx, "StaticInt", "wholePipeline", "(II)I", &[3, 4])?;

    let limits = CollectionLimits {
        objects: 32,
        edges: 32,
        stack: 32,
        work: 256,
        clears: 32,
        finalizers: 0,
    };
    let mut heap =
        JvmHeap::new(32, limits).map_err(|error| sim_kernel::Error::Eval(error.to_string()))?;
    let object_allocated = heap.allocate(JvmRole::Object).is_ok();
    let make_throwable = |condition| throwable(cx, condition);
    let mut array = JavaArray::allocate(
        &mut heap,
        ArrayComponent::Primitive(ArrayPrimitive::Int),
        2,
        make_throwable,
    )
    .map_err(|error| sim_kernel::Error::Eval(format!("array allocation failed: {error:?}")))?;
    array
        .store_primitive(1, JvmValue::Int(17), |condition| throwable(cx, condition))
        .map_err(|error| sim_kernel::Error::Eval(format!("array store failed: {error:?}")))?;
    let array_result = match array
        .load(1, |condition| throwable(cx, condition))
        .map_err(|error| sim_kernel::Error::Eval(format!("array load failed: {error:?}")))?
    {
        JvmValue::Int(value) => *value,
        _ => {
            return Err(sim_kernel::Error::Eval(
                "JVM int array changed category".into(),
            ));
        }
    };
    let exception_class = FailureCondition::NegativeArraySize
        .java_class()
        .expect("negative array size is a guest exception");
    let exception = JavaArray::allocate(
        &mut heap,
        ArrayComponent::Primitive(ArrayPrimitive::Int),
        -1,
        |condition| throwable(cx, condition),
    );
    if !matches!(exception, Err(crate::ArrayAllocationError::Java(_))) {
        return Err(sim_kernel::Error::Eval(
            "negative array did not raise".into(),
        ));
    }
    let left = JavaString::new(CodeUnitString::from_scalar("SIM "));
    let right = JavaString::new(CodeUnitString::from_scalar("JVM"));
    let concat = left.concat(&right)?;
    let concat_result = String::from_utf16(&concat.storage().code_units().collect::<Vec<_>>())
        .map_err(|_| sim_kernel::Error::Eval("specimen concat is not scalar text".into()))?;

    Ok(JvmProductSpecimen {
        static_result,
        object_allocated,
        array_result,
        exception_class,
        concat_result,
    })
}

fn throwable(cx: &Cx, condition: FailureCondition) -> JavaThrowable {
    let class = condition.java_class().expect("throwable-owned condition");
    let raised = Raised::new(
        cx.factory()
            .symbol(Symbol::new(class))
            .expect("symbol value"),
        cx.factory()
            .string(class.to_owned())
            .expect("message value"),
        Origin {
            codec: CodecId(0),
            source: SourceId("jvm-product-specimen".into()),
            span: Span { start: 0, end: 0 },
            trivia: vec![],
        },
        Symbol::new("java/jvm"),
    )
    .expect("raised envelope");
    JavaThrowable::new(condition, raised).expect("throwable-owned condition")
}
