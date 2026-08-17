// This conformance module intentionally exercises the public by-value `Raised`
// carrier; boxing it would replace the guest-runtime boundary under test.
#![allow(clippy::result_large_err)]

use std::{
    any::Any,
    sync::{Arc, Mutex},
};

use sim_kernel::{
    Args, Callable, ClassId, ClassRef, CodecId, Cx, DefaultFactory, EagerPolicy, Expr, Object,
    ObjectCompat, Origin, Result, ShapeRef, SourceId, Span, Symbol, Value,
};
use sim_lib_control::{
    BoundedSubclassOutcome, ClassMatchBudget, ClassMatchEvidence, ClassMatchOutcome, CleanupStack,
    ProtectedOutcome, Raised, RaisedBrowseBudget, RaisedUnwind, match_raised_class,
};
use sim_lib_lang_javascript::{
    JavascriptExceptionRealm, JavascriptHeap, JavascriptObjects, JavascriptPropertyKey,
    JavascriptValue,
};
use sim_lib_lang_lua::LuaExceptionProfile;
use sim_lib_lang_python::{PythonExceptionRelation, PythonExceptions};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LookupModel {
    DeclaredParents,
    Prototype,
    MetaIndex,
}

struct GuestFunction {
    language: &'static str,
    lookup: LookupModel,
    args: ShapeRef,
    result: ShapeRef,
}

impl Object for GuestFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<{} function>", self.language))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl ObjectCompat for GuestFunction {
    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(Symbol::qualified(self.language, "answer")))
    }

    fn truth(&self, _cx: &mut Cx) -> Result<bool> {
        Ok(true)
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for GuestFunction {
    fn call(&self, cx: &mut Cx, _args: Args) -> Result<Value> {
        cx.factory().symbol(Symbol::new("outcome/42"))
    }

    fn browse_args_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(Some(self.args.clone()))
    }

    fn browse_result_shape(&self, _cx: &mut Cx) -> Result<Option<ShapeRef>> {
        Ok(Some(self.result.clone()))
    }
}

fn specimen_cx() -> Cx {
    Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory))
}

fn class(cx: &Cx, id: u32, language: &str, name: &str) -> ClassRef {
    cx.factory()
        .class_stub(ClassId(id), Symbol::qualified(language, name))
        .unwrap()
}

fn origin(language: &str, at: usize) -> Origin {
    Origin {
        codec: CodecId(1),
        source: SourceId(format!("exceptions3-{language}")),
        span: Span {
            start: at,
            end: at + 1,
        },
        trivia: Vec::new(),
    }
}

fn exact_match(cx: &mut Cx, raised: &Raised) -> ClassMatchOutcome {
    let candidate = raised.class_ref().clone();
    match_raised_class(
        cx,
        raised,
        candidate,
        ClassMatchBudget { work: 1 },
        |_, actual, expected, _| {
            let raised = actual.object().as_class().unwrap().id();
            let candidate = expected.object().as_class().unwrap().id();
            BoundedSubclassOutcome::Subclass(ClassMatchEvidence {
                raised,
                candidate,
                performed_work: 1,
            })
        },
        |_, _, _| Ok(true),
    )
}

fn lua_protected<T>(
    body: impl FnOnce() -> std::result::Result<T, Raised>,
) -> ProtectedOutcome<Raised> {
    match body() {
        Ok(_) => unreachable!("exception specimen must raise"),
        Err(raised) => ProtectedOutcome::Raised(raised),
    }
}

#[test]
fn python_javascript_and_lua_converge_only_at_kernel_protocols() {
    let mut cx = specimen_cx();
    let args: ShapeRef = cx
        .factory()
        .symbol(Symbol::new("shape/guest-args"))
        .unwrap();
    let result: ShapeRef = cx
        .factory()
        .symbol(Symbol::new("shape/guest-result"))
        .unwrap();
    let functions = [
        GuestFunction {
            language: "python",
            lookup: LookupModel::DeclaredParents,
            args: args.clone(),
            result: result.clone(),
        },
        GuestFunction {
            language: "javascript",
            lookup: LookupModel::Prototype,
            args: args.clone(),
            result: result.clone(),
        },
        GuestFunction {
            language: "lua",
            lookup: LookupModel::MetaIndex,
            args,
            result,
        },
    ];

    let mut outcomes = Vec::new();
    for function in &functions {
        let callable = function
            .as_callable()
            .expect("every guest function projects the kernel Callable protocol");
        assert!(callable.browse_args_shape(&mut cx).unwrap().is_some());
        assert!(callable.browse_result_shape(&mut cx).unwrap().is_some());
        outcomes.push(callable.call(&mut cx, Args::default()).unwrap());
    }

    let canonical = outcomes
        .iter()
        .map(|value| value.object().as_expr(&mut cx).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(canonical, vec![canonical[0].clone(); 3]);
    assert_eq!(functions[0].lookup, LookupModel::DeclaredParents);
    assert_eq!(functions[1].lookup, LookupModel::Prototype);
    assert_eq!(functions[2].lookup, LookupModel::MetaIndex);
    assert_eq!(
        functions
            .iter()
            .filter(|function| function.lookup == LookupModel::DeclaredParents)
            .map(|function| function.language)
            .collect::<Vec<_>>(),
        ["python"]
    );
}

#[test]
fn raised_carriage_is_shared_while_guest_relations_remain_distinct() {
    let mut cx = specimen_cx();
    let python_error = class(&cx, 101, "python", "ValueError");
    let python_group = class(&cx, 102, "python", "ExceptionGroup");
    let mut python = PythonExceptions::new(8).unwrap();
    python
        .define_class(&cx, python_error.clone(), Vec::new())
        .unwrap();
    python
        .define_class(&cx, python_group.clone(), Vec::new())
        .unwrap();
    let member = python
        .allocate(python_error.clone(), "python boom", origin("python", 1))
        .unwrap();
    let group = python
        .group(python_group, "python group", &[member], origin("python", 2))
        .unwrap();
    let python_raised = python.raise(&cx, group).unwrap();

    let javascript_aggregate = class(&cx, 207, "javascript", "AggregateError");
    let mut javascript_objects = JavascriptObjects::new(JavascriptHeap::retaining(4).unwrap());
    let aggregate = javascript_objects.ordinary().unwrap();
    let aggregate_members = javascript_objects.ordinary().unwrap();
    javascript_objects
        .define_data(
            aggregate_members,
            JavascriptPropertyKey::String("0".into()),
            JavascriptValue::String("js boom".into()),
            true,
            true,
            true,
        )
        .unwrap();
    javascript_objects
        .define_data(
            aggregate,
            JavascriptPropertyKey::String("errors".into()),
            JavascriptValue::Managed(aggregate_members),
            true,
            false,
            true,
        )
        .unwrap();
    let mut javascript = JavascriptExceptionRealm::new(
        class(&cx, 201, "javascript", "Undefined"),
        class(&cx, 202, "javascript", "Null"),
        class(&cx, 203, "javascript", "Boolean"),
        class(&cx, 204, "javascript", "Number"),
        class(&cx, 205, "javascript", "BigInt"),
        class(&cx, 206, "javascript", "String"),
        javascript_aggregate,
    );
    javascript.register_managed_class(aggregate, class(&cx, 207, "javascript", "AggregateError"));
    let javascript_raised = javascript
        .raise(
            &cx,
            JavascriptValue::Managed(aggregate),
            origin("javascript", 1),
        )
        .unwrap();

    let lua = LuaExceptionProfile::new(&cx).unwrap();
    let lua_payload = cx.factory().string("lua boom".to_owned()).unwrap();
    let lua_raised = lua.raise(lua_payload, origin("lua", 1)).unwrap();

    let cleanup_log = Arc::new(Mutex::new(Vec::new()));
    let raised = [python_raised.clone(), javascript_raised, lua_raised];
    let mut output = Vec::new();
    for (language, raised) in ["python", "javascript", "lua"].into_iter().zip(raised) {
        assert!(matches!(
            exact_match(&mut cx, &raised),
            ClassMatchOutcome::Matched(_)
        ));
        let browse = raised
            .browse(&mut cx, RaisedBrowseBudget::new(128).unwrap())
            .unwrap();
        assert!(!browse.truncated);
        let mut cleanup: CleanupStack<RaisedUnwind<(), (), ()>> = CleanupStack::new();
        let log = cleanup_log.clone();
        cleanup.push(move |_| log.lock().unwrap().push(language));
        let RaisedUnwind::Exception(raised) = cleanup.unwind(RaisedUnwind::Exception(raised))
        else {
            unreachable!()
        };
        output.push(format!(
            "{language}: matched=true class={} browse={} cleanup=done",
            raised.class_ref().object().display(&mut cx).unwrap(),
            browse.payload
        ));
    }
    assert_eq!(
        *cleanup_log.lock().unwrap(),
        ["python", "javascript", "lua"]
    );

    let python_projection = python.relations(group).unwrap();
    assert_eq!(
        python_projection
            .iter()
            .map(|(relation, _)| *relation)
            .collect::<Vec<_>>(),
        [PythonExceptionRelation::GroupMember(0)]
    );
    let aggregate_projection = ["errors[0]"];
    assert_eq!(
        javascript_objects
            .get(
                aggregate_members,
                &JavascriptPropertyKey::String("0".into()),
                1,
            )
            .unwrap(),
        Some(JavascriptValue::String("js boom".into()))
    );
    let suppressed_projection = ["suppressed[0]"];
    assert_ne!(
        format!("{:?}", python_projection[0].0),
        aggregate_projection[0]
    );
    assert_ne!(aggregate_projection, suppressed_projection);
    assert_ne!(
        format!("{:?}", python_projection[0].0),
        suppressed_projection[0]
    );
    output.push("relations: python=[GroupMember(0)] javascript=[errors[0]] java=[suppressed[0]] distinct=true".into());

    let ProtectedOutcome::Raised(caught) = lua_protected(|| Err::<(), _>(python_raised.clone()))
    else {
        unreachable!()
    };
    assert_eq!(caught.class_ref(), python_raised.class_ref());
    assert_eq!(python.relations(group).unwrap(), python_projection);
    output.push("lua-protected: python-class=preserved python-relations=preserved".into());

    println!("{}", output.join("\n"));
}
