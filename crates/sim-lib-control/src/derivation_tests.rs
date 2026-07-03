use std::sync::Arc;

use sim_kernel::{
    Cx, Datum, DatumStore, DefaultFactory, NoopEvalPolicy, Ref, Symbol,
    capability::{control_capture_capability, control_resume_capability},
    control::{
        control_aborted_status, control_captured_status, control_result_status,
        control_resumed_status,
    },
};

use super::*;

fn cx() -> Cx {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    install_control_policy(&mut cx);
    cx
}

fn symbol_ref(namespace: &str, name: &str) -> Ref {
    Ref::Symbol(Symbol::qualified(namespace, name))
}

#[test]
fn condition_signal_uses_nearest_dynamic_handler_and_control_capture() {
    let mut cx = cx();
    cx.grant(control_capture_capability());
    let condition_kind = Symbol::qualified("condition", "warning");
    let mut stack = ConditionStack::new();
    stack.push(ConditionHandler::new(
        condition_kind.clone(),
        symbol_ref("prompt", "outer"),
        symbol_ref("handler", "outer"),
    ));
    stack.push(ConditionHandler::new(
        Symbol::qualified("condition", "other"),
        symbol_ref("prompt", "other"),
        symbol_ref("handler", "other"),
    ));
    stack.push(ConditionHandler::new(
        condition_kind.clone(),
        symbol_ref("prompt", "inner"),
        symbol_ref("handler", "inner"),
    ));

    let continuation = signal_condition(
        &mut cx,
        &stack,
        Condition::new(condition_kind, symbol_ref("payload", "warning")),
    )
    .unwrap();

    assert_eq!(continuation.continuation(), &symbol_ref("handler", "inner"));
    assert_eq!(
        control_result_status(&cx, continuation.capture_result()).unwrap(),
        Some(control_captured_status())
    );
    assert_eq!(cx.effect_ledger().records().len(), 1);
}

#[test]
fn restart_resumes_at_signaling_site() {
    let mut cx = cx();
    cx.grant(control_capture_capability());
    cx.grant(control_resume_capability());
    let condition_kind = Symbol::qualified("condition", "missing-value");
    let restart_name = Symbol::qualified("restart", "use-value");
    let mut handlers = ConditionStack::new();
    handlers.push(ConditionHandler::new(
        condition_kind.clone(),
        symbol_ref("prompt", "condition"),
        symbol_ref("site", "signal"),
    ));
    let continuation = handlers
        .signal(
            &mut cx,
            Condition::new(condition_kind, symbol_ref("payload", "missing")),
        )
        .unwrap();
    let mut restarts = RestartStack::new();
    restarts.push(Restart::new(restart_name.clone(), continuation));

    let result = invoke_restart(
        &mut cx,
        &restarts,
        &restart_name,
        symbol_ref("value", "replacement"),
    )
    .unwrap();

    assert_eq!(
        control_result_status(&cx, result.reference()).unwrap(),
        Some(control_resumed_status())
    );
    assert_eq!(
        control_result_symbol_field(&cx, result.reference(), "continuation"),
        Some(Symbol::qualified("site", "signal"))
    );
}

#[test]
fn generator_yields_finite_sequence_and_reports_exhaustion() {
    let mut generator = Generator::new(vec![
        symbol_ref("item", "one"),
        symbol_ref("item", "two"),
        symbol_ref("item", "three"),
    ]);

    assert_eq!(
        generator.next_step(),
        GeneratorStep::Yielded(symbol_ref("item", "one"))
    );
    assert_eq!(
        generator.next_step(),
        GeneratorStep::Yielded(symbol_ref("item", "two"))
    );
    assert_eq!(
        generator.next_step(),
        GeneratorStep::Yielded(symbol_ref("item", "three"))
    );
    assert_eq!(generator.next_step(), GeneratorStep::Exhausted);
    assert!(generator.is_exhausted());
}

#[test]
fn coroutine_alternates_deterministically() {
    let mut coroutine = Coroutine::alternating(
        vec![symbol_ref("first", "one"), symbol_ref("first", "two")],
        vec![symbol_ref("second", "one"), symbol_ref("second", "two")],
    );

    assert_eq!(
        coroutine.resume(),
        CoroutineStep::Yielded {
            lane: CoroutineLane::First,
            value: symbol_ref("first", "one")
        }
    );
    assert_eq!(
        coroutine.resume(),
        CoroutineStep::Yielded {
            lane: CoroutineLane::Second,
            value: symbol_ref("second", "one")
        }
    );
    assert_eq!(
        coroutine.resume(),
        CoroutineStep::Yielded {
            lane: CoroutineLane::First,
            value: symbol_ref("first", "two")
        }
    );
    assert_eq!(
        coroutine.resume(),
        CoroutineStep::Yielded {
            lane: CoroutineLane::Second,
            value: symbol_ref("second", "two")
        }
    );
    assert_eq!(coroutine.resume(), CoroutineStep::Exhausted);
}

#[test]
fn break_next_and_return_escape_to_matching_labeled_prompt() {
    let label = Symbol::qualified("label", "loop");
    let exits = [
        NonLocalExit::break_to(label.clone(), symbol_ref("value", "break")),
        NonLocalExit::next_to(label.clone(), symbol_ref("value", "next")),
        NonLocalExit::return_to(label.clone(), symbol_ref("value", "return")),
    ];

    for exit in exits {
        let mut cx = cx();
        cx.grant(control_capture_capability());
        let prompts = vec![
            LabeledPrompt::new(label.clone(), symbol_ref("prompt", "outer")),
            LabeledPrompt::new(
                Symbol::qualified("label", "other"),
                symbol_ref("prompt", "other"),
            ),
            LabeledPrompt::new(label.clone(), symbol_ref("prompt", "inner")),
        ];

        let result = escape_to_label(&mut cx, &prompts, exit).unwrap();

        assert_eq!(
            control_result_status(&cx, result.reference()).unwrap(),
            Some(control_aborted_status())
        );
        assert_eq!(
            control_result_symbol_field(&cx, result.reference(), "prompt"),
            Some(Symbol::qualified("prompt", "inner"))
        );
    }
}

#[test]
fn async_task_reports_pending_then_ready() {
    let mut task = AsyncTask::ready_after(2, symbol_ref("async", "ready"));

    assert_eq!(task.poll(), AsyncPoll::Pending);
    assert_eq!(task.poll(), AsyncPoll::Pending);
    assert_eq!(task.poll(), AsyncPoll::Ready(symbol_ref("async", "ready")));
}

#[test]
fn backtracker_walks_choices_and_then_fails_closed() {
    let mut backtracker = Backtracker::new(vec![
        symbol_ref("choice", "first"),
        symbol_ref("choice", "second"),
    ]);

    assert_eq!(
        backtracker.choose(),
        BacktrackStep::Choice(symbol_ref("choice", "first"))
    );
    assert_eq!(
        backtracker.fail(),
        BacktrackStep::Choice(symbol_ref("choice", "second"))
    );
    assert_eq!(backtracker.fail(), BacktrackStep::Failed);
}

fn control_result_symbol_field(cx: &Cx, result: &Ref, field_name: &str) -> Option<Symbol> {
    let Ref::Content(id) = result else {
        return None;
    };
    let Some(Datum::Node { fields, .. }) = cx.datum_store().get(id).unwrap() else {
        return None;
    };
    let field = fields
        .iter()
        .find_map(|(name, value)| (name == &Symbol::new(field_name)).then_some(value))?;
    let Datum::Node {
        fields: ref_fields, ..
    } = field
    else {
        return None;
    };
    ref_fields.iter().find_map(|(name, value)| {
        if name == &Symbol::new("symbol")
            && let Datum::Symbol(symbol) = value
        {
            return Some(symbol.clone());
        }
        None
    })
}
