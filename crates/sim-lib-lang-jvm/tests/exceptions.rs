use std::sync::{Arc, Mutex};

use sim_codec_classfile::{ByteReader, CodeException, ConstantPool, Opcode, decode_instructions};
use sim_kernel::{
    Args, Callable, Class, ClassId, ClassRef, CodecId, Cx, Error, Object, ObjectCompat, Origin,
    ReadConstructorRef, Result, ShapeRef, SourceId, Span, Symbol, TableRef, Value,
};
use sim_lib_control::{
    BoundedSubclassOutcome, ClassMatchBudget, ClassMatchEvidence, CleanupStack, Raised,
    RaisedUnwind, WorkLimit,
};
use sim_lib_gc_tracing::CollectionLimits;
use sim_lib_lang_jvm::{
    JavaThrowError, JavaThrowSite, JavaThrowableHeap, JavaThrowableMutationError,
    JavaThrowableRelation, JvmInstructionPolicy, JvmInstructionSemantics, JvmSlotKind, JvmValue,
    JvmValueWidth, execute_athrow, prepare_code, unwind_java_frame,
};
use sim_lib_machine::UnitStack;

const NONE: &[JvmSlotKind] = &[];

struct Policy;
impl JvmInstructionPolicy for Policy {
    fn semantics(_: Opcode) -> Option<JvmInstructionSemantics> {
        Some(JvmInstructionSemantics {
            pops: NONE,
            pushes: NONE,
            safepoint: false,
        })
    }
}

struct TestClass(ClassId, &'static str);
impl Object for TestClass {
    fn display(&self, _: &mut Cx) -> Result<String> {
        Ok(self.1.into())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
impl ObjectCompat for TestClass {
    fn class(&self, _: &mut Cx) -> Result<ClassRef> {
        Err(Error::Eval("unused".into()))
    }
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
    fn as_class(&self) -> Option<&dyn Class> {
        Some(self)
    }
}
impl Callable for TestClass {
    fn call(&self, _: &mut Cx, _: Args) -> Result<Value> {
        Err(Error::Eval("unused".into()))
    }
}
impl Class for TestClass {
    fn id(&self) -> ClassId {
        self.0
    }
    fn symbol(&self) -> Symbol {
        Symbol::qualified("java", self.1)
    }
    fn constructor_shape(&self, _: &mut Cx) -> Result<ShapeRef> {
        Err(Error::Eval("unused".into()))
    }
    fn instance_shape(&self, _: &mut Cx) -> Result<ShapeRef> {
        Err(Error::Eval("unused".into()))
    }
    fn read_constructor(&self, _: &mut Cx) -> Result<Option<ReadConstructorRef>> {
        Ok(None)
    }
    fn members(&self, _: &mut Cx) -> Result<TableRef> {
        Err(Error::Eval("unused".into()))
    }
}

fn collection_limits() -> CollectionLimits {
    CollectionLimits {
        objects: 64,
        edges: 64,
        stack: 64,
        work: 512,
        clears: 64,
        finalizers: 64,
    }
}

fn raised(cx: &mut Cx, id: u32, name: &'static str) -> Raised {
    let class = cx
        .factory()
        .opaque(Arc::new(TestClass(ClassId(id), name)))
        .unwrap();
    assert!(
        class.object().as_class().is_some(),
        "throwable class must have a CLASS_2 face"
    );
    Raised::new(
        class,
        cx.factory().string(name.into()).unwrap(),
        Origin {
            codec: CodecId(0),
            source: SourceId("exceptions".into()),
            span: Span { start: 0, end: 1 },
            trivia: vec![],
        },
        Symbol::new("java/jvm"),
    )
    .unwrap()
}

fn prepared() -> sim_lib_lang_jvm::PreparedJvmInstruction {
    let pool = ConstantPool::decode(&mut ByteReader::new(&[0, 1], 1), 61).unwrap();
    let bytes = [Opcode::Athrow as u8, Opcode::Nop as u8, Opcode::Nop as u8];
    let decoded = decode_instructions(&bytes, 61, &pool).unwrap();
    let handlers = [
        CodeException {
            start_pc: 0,
            end_pc: 1,
            handler_pc: 1,
            catch_type: 1,
        },
        CodeException {
            start_pc: 0,
            end_pc: 1,
            handler_pc: 2,
            catch_type: 0,
        },
    ];
    let code =
        prepare_code::<Policy>(&decoded, bytes.len(), &handlers, SourceId("try".into())).unwrap();
    code.instruction(code.entry()).instruction().clone()
}

#[test]
fn nested_try_cleanup_rethrow_uses_ordered_handlers_and_exact_entry_stack() {
    let mut cx = sim_kernel::testing::bare_cx();
    let thrown = raised(&mut cx, 20, "Leaf");
    let candidate = cx
        .factory()
        .opaque(Arc::new(TestClass(ClassId(10), "Parent")))
        .unwrap();
    let mut heap = JavaThrowableHeap::new(8, collection_limits()).unwrap();
    let handle = heap.allocate(thrown.clone()).unwrap();
    let mut stack = UnitStack::<JvmValueWidth>::new(WorkLimit(8));
    stack.push(JvmValue::Int(99)).unwrap();
    stack
        .push(JvmValue::Reference(
            sim_lib_lang_jvm::JvmReference::managed(handle),
        ))
        .unwrap();
    let entry = execute_athrow(
        &mut cx,
        JavaThrowSite {
            instruction: &prepared(),
            operands: &mut stack,
        },
        &thrown,
        ClassMatchBudget { work: 8 },
        |_| Some(candidate.clone()),
        |_, raised, candidate, _| {
            BoundedSubclassOutcome::Subclass(ClassMatchEvidence {
                raised: raised.object().as_class().unwrap().id(),
                candidate: candidate.object().as_class().unwrap().id(),
                performed_work: 2,
            })
        },
        |_, _, _| Ok(false),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        entry.row, 1,
        "the narrowed first row must not eclipse the later catch-all"
    );
    assert_eq!(stack.depth(), 1);
    assert!(
        matches!(stack.pop().unwrap(), JvmValue::Reference(reference) if reference.handle() == Some(handle))
    );

    let log = Arc::new(Mutex::new(Vec::new()));
    for extent in ["inner", "outer"] {
        let mut cleanups: CleanupStack<RaisedUnwind<(), (), ()>> = CleanupStack::new();
        let log = Arc::clone(&log);
        cleanups.push(move |_| log.lock().unwrap().push(extent));
        let _ = unwind_java_frame(thrown.clone(), cleanups);
    }
    assert_eq!(*log.lock().unwrap(), ["inner", "outer"]);
}

#[test]
fn java_relations_refuse_self_suppression_preserve_order_and_collect_cause_cycles() {
    let mut cx = sim_kernel::testing::bare_cx();
    let mut heap = JavaThrowableHeap::new(8, collection_limits()).unwrap();
    let a = heap.allocate(raised(&mut cx, 1, "A")).unwrap();
    let b = heap.allocate(raised(&mut cx, 2, "B")).unwrap();
    assert!(matches!(
        heap.add_suppressed(a, a),
        Err(JavaThrowableMutationError::SelfSuppression)
    ));
    heap.add_suppressed(a, b).unwrap();
    heap.init_cause(a, b).unwrap();
    heap.init_cause(b, a).unwrap();
    assert_eq!(
        heap.relations(a).unwrap(),
        vec![
            (JavaThrowableRelation::Suppressed, b.id()),
            (JavaThrowableRelation::Cause, b.id()),
        ]
    );
    assert_eq!(heap.live_len(), 2);
    heap.collect().unwrap();
    assert_eq!(
        heap.live_len(),
        0,
        "an unreachable Java cause cycle must be collectible"
    );
}

#[test]
fn athrow_null_is_distinct_from_host_failure() {
    let mut cx = sim_kernel::testing::bare_cx();
    let thrown = raised(&mut cx, 20, "Leaf");
    let mut stack = UnitStack::<JvmValueWidth>::new(WorkLimit(1));
    stack
        .push(JvmValue::Reference(sim_lib_lang_jvm::JvmReference::NULL))
        .unwrap();
    assert!(matches!(
        execute_athrow(
            &mut cx,
            JavaThrowSite {
                instruction: &prepared(),
                operands: &mut stack
            },
            &thrown,
            ClassMatchBudget { work: 1 },
            |_| None,
            |_, _, _, _| unreachable!(),
            |_, _, _| Ok(true)
        ),
        Err(JavaThrowError::NullReference)
    ));
}
