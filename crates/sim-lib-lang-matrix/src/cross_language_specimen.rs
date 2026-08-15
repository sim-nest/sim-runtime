use std::{any::Any, sync::Arc};

use sim_kernel::{
    Args, Callable, Cx, DefaultFactory, EagerPolicy, Expr, Object, ObjectCompat, Result, ShapeRef,
    Symbol, Value,
};

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
