// conformance: function instances compose with ordinary callable dispatch.

//! Optional composition between ordinary function instances and dispatch.

use std::sync::Arc;

use sim_lib_dispatch::MethodBody;

use crate::{FunctionBodyPolicy, FunctionInstance};

/// Projects a function instance into a dispatch method body.
///
/// This adapter is deliberately opt-in: [`FunctionInstance`] remains a kernel
/// callable in its own right, and an ordinary call never constructs or consults
/// a generic function. Dispatch supplies only method selection; after selection,
/// the same neutral call boundary invokes the same guest policy.
pub fn dispatch_method_body<B>(function: FunctionInstance<B>) -> MethodBody
where
    B: FunctionBodyPolicy,
{
    Arc::new(move |cx, arguments| function.invoke_values(cx, arguments.to_vec()))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use sim_kernel::{
        Args, Callable, ClassRef, Cx, Expr, Result, Shape, Symbol, Value,
        shape::{MatchScore, ShapeDoc, ShapeMatch},
        testing::bare_cx,
    };
    use sim_lib_dispatch::{DispatchMethod, GenericFunction, MethodRole};

    use super::*;
    use crate::{ArgumentInput, ArgumentOrigin, BoundCall, FunctionPlan};

    type RecordedArguments = Vec<(ArgumentOrigin, Value)>;
    type RecordedCalls = Arc<Mutex<Vec<RecordedArguments>>>;

    #[derive(Clone)]
    struct RecordingBody {
        calls: RecordedCalls,
    }

    impl FunctionBodyPolicy for RecordingBody {
        fn invoke(
            &self,
            _cx: &mut Cx,
            _plan: &FunctionPlan,
            _captures: &[crate::CapturedBinding],
            call: BoundCall,
        ) -> Result<Value> {
            let received = call
                .arguments()
                .iter()
                .map(|argument| match argument.input() {
                    ArgumentInput::Positional(value) => (argument.origin().clone(), value.clone()),
                    _ => unreachable!("evaluated call plans contain positional inputs"),
                })
                .collect::<Vec<_>>();
            let result = received[0].1.clone();
            self.calls.lock().unwrap().push(received);
            Ok(result)
        }
    }

    struct InstrumentedShape {
        selections: Arc<AtomicUsize>,
    }

    impl Shape for InstrumentedShape {
        fn check_value(&self, _cx: &mut Cx, _value: Value) -> Result<ShapeMatch> {
            self.selections.fetch_add(1, Ordering::SeqCst);
            Ok(ShapeMatch::accept(MatchScore::exact(0)))
        }

        fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> Result<ShapeMatch> {
            Ok(ShapeMatch::accept(MatchScore::exact(0)))
        }

        fn describe(&self, _cx: &mut Cx) -> Result<ShapeDoc> {
            Ok(ShapeDoc::new("instrumented"))
        }
    }

    fn instance(cx: &mut Cx, calls: RecordedCalls) -> FunctionInstance<RecordingBody> {
        let class: ClassRef = cx.factory().symbol(Symbol::new("guest-function")).unwrap();
        FunctionInstance::new(
            FunctionPlan::new(Symbol::new("guest:record"), vec![], vec![], None).unwrap(),
            RecordingBody { calls },
            vec![],
            class,
            None,
            None,
        )
        .unwrap()
    }

    #[test]
    fn direct_and_selected_invocation_deliver_identical_bound_calls() {
        let mut cx = bare_cx();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let selections = Arc::new(AtomicUsize::new(0));
        let function = instance(&mut cx, calls.clone());
        let argument = cx.factory().symbol(Symbol::new("argument")).unwrap();

        function
            .call(&mut cx, Args::new(vec![argument.clone()]))
            .unwrap();
        assert_eq!(selections.load(Ordering::SeqCst), 0);

        let mut generic = GenericFunction::new(Symbol::new("guest:generic"));
        generic
            .add_method(DispatchMethod::new(
                Symbol::new("guest:method"),
                MethodRole::Primary,
                vec![Arc::new(InstrumentedShape {
                    selections: selections.clone(),
                })],
                dispatch_method_body(function),
            ))
            .unwrap();
        generic.call(&mut cx, &[argument]).unwrap();

        assert_eq!(selections.load(Ordering::SeqCst), 1);
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].len(), calls[1].len());
        for (direct, selected) in calls[0].iter().zip(&calls[1]) {
            assert_eq!(direct.0, selected.0);
            assert_eq!(direct.1, selected.1);
        }
        assert_eq!(calls[0][0].0, ArgumentOrigin::KernelPosition(0));
    }
}
