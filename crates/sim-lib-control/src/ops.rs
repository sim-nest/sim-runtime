use std::sync::Arc;

use sim_kernel::{
    Args, Callable, ClassRef, Cx, Error, Expr, Object, ObjectCompat, RawArgs, Ref, Result, Symbol,
    Value,
    control::{
        ControlAbort, ControlCapture, ControlPrompt, ControlResume, abort, capture,
        default_control_result_shape, prompt, resume,
    },
};

use crate::model::{ContinuationValue, ControlResultValue};

/// A callable runtime object exposing one control primitive.
///
/// The core [`ControlFunction`] variants (`prompt`, `capture`, `abort`,
/// `resume`) are installed by the control lib as `control/*` functions, turning
/// the kernel control-policy operations into callables the runtime can invoke.
#[derive(Clone)]
pub struct ControlFunction {
    kind: ControlFunctionKind,
}

#[derive(Clone, Copy)]
enum ControlFunctionKind {
    Prompt,
    Capture,
    Abort,
    Resume,
    PhysicalSensingTrace,
}

impl ControlFunction {
    /// Builds the `control/prompt` function, which establishes a prompt.
    pub fn prompt() -> Self {
        Self {
            kind: ControlFunctionKind::Prompt,
        }
    }

    /// Builds the `control/capture` function, which captures a continuation.
    pub fn capture() -> Self {
        Self {
            kind: ControlFunctionKind::Capture,
        }
    }

    /// Builds the `control/abort` function, which aborts to a prompt.
    pub fn abort() -> Self {
        Self {
            kind: ControlFunctionKind::Abort,
        }
    }

    /// Builds the `control/resume` function, which resumes a continuation.
    pub fn resume() -> Self {
        Self {
            kind: ControlFunctionKind::Resume,
        }
    }

    /// Builds the deterministic physical-sensing descriptor fixture.
    pub fn physical_sensing_trace() -> Self {
        Self {
            kind: ControlFunctionKind::PhysicalSensingTrace,
        }
    }

    /// Returns the `control/*` symbol under which this function is exported.
    pub fn symbol(&self) -> Symbol {
        self.kind.symbol()
    }
}

impl Object for ControlFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<function {}>", self.kind.symbol()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ControlFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for ControlFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        self.kind.call(cx, args.into_vec())
    }

    fn call_exprs(&self, cx: &mut Cx, args: RawArgs) -> Result<Value> {
        let values = args
            .into_exprs()
            .into_iter()
            .map(|expr| cx.eval_expr(expr))
            .collect::<Result<Vec<_>>>()?;
        self.kind.call(cx, values)
    }
}

impl ControlFunctionKind {
    fn symbol(self) -> Symbol {
        match self {
            Self::Prompt => prompt_symbol(),
            Self::Capture => capture_symbol(),
            Self::Abort => abort_symbol(),
            Self::Resume => resume_symbol(),
            Self::PhysicalSensingTrace => physical_sensing_trace_symbol(),
        }
    }

    fn call(self, cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
        match self {
            Self::Prompt => call_prompt(cx, args),
            Self::Capture => call_capture(cx, args),
            Self::Abort => call_abort(cx, args),
            Self::Resume => call_resume(cx, args),
            Self::PhysicalSensingTrace => call_physical_sensing_trace(cx, args),
        }
    }
}

/// Returns the `control/prompt` symbol.
pub fn prompt_symbol() -> Symbol {
    Symbol::qualified("control", "prompt")
}

/// Returns the `control/capture` symbol.
pub fn capture_symbol() -> Symbol {
    Symbol::qualified("control", "capture")
}

/// Returns the `control/abort` symbol.
pub fn abort_symbol() -> Symbol {
    Symbol::qualified("control", "abort")
}

/// Returns the `control/resume` symbol.
pub fn resume_symbol() -> Symbol {
    Symbol::qualified("control", "resume")
}

/// Returns the `control/physical-sensing-trace` fixture symbol.
pub fn physical_sensing_trace_symbol() -> Symbol {
    Symbol::qualified("control", "physical-sensing-trace")
}

fn call_prompt(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let refs = refs_from_args(cx, args, "control/prompt")?;
    let [prompt_ref, value_ref] = refs.as_slice() else {
        return Err(arity_error("control/prompt", "prompt value"));
    };
    let prompt_ref = prompt_ref.clone();
    let value_ref = value_ref.clone();
    let result = prompt(
        cx,
        ControlPrompt::new(
            prompt_ref,
            value_ref.clone(),
            default_control_result_shape(),
        ),
        |_cx| Ok(value_ref),
    )?;
    control_result_value(cx, result)
}

fn call_capture(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let multishot = optional_bool_arg(cx, args.get(3))?;
    let refs = refs_from_args(cx, args.into_iter().take(3).collect(), "control/capture")?;
    let [prompt_ref, continuation_ref, value_ref] = refs.as_slice() else {
        return Err(arity_error(
            "control/capture",
            "prompt continuation value [multishot]",
        ));
    };
    let mut request = ControlCapture::new(
        prompt_ref.clone(),
        continuation_ref.clone(),
        value_ref.clone(),
        default_control_result_shape(),
    );
    if multishot {
        request = request.multishot();
    }
    let capture_result = capture(cx, request)?;
    cx.factory().opaque(Arc::new(ContinuationValue::new(
        continuation_ref.clone(),
        capture_result,
        multishot,
    )))
}

fn call_abort(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    let refs = refs_from_args(cx, args, "control/abort")?;
    let [prompt_ref, value_ref] = refs.as_slice() else {
        return Err(arity_error("control/abort", "prompt value"));
    };
    let prompt_ref = prompt_ref.clone();
    let value_ref = value_ref.clone();
    let result = abort(
        cx,
        ControlAbort::new(prompt_ref, value_ref, default_control_result_shape()),
    )?;
    control_result_value(cx, result)
}

fn call_resume(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    if args.len() != 2 {
        return Err(arity_error("control/resume", "continuation value"));
    }
    let continuation = continuation_ref(cx, &args[0])?;
    let value = value_ref(cx, &args[1], "control/resume value")?;
    let result = resume(
        cx,
        ControlResume::new(continuation, value, default_control_result_shape()),
    )?;
    control_result_value(cx, result)
}

fn call_physical_sensing_trace(cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
    if !args.is_empty() {
        return Err(arity_error(
            "control/physical-sensing-trace",
            "no arguments",
        ));
    }
    cx.factory().expr(physical_sensing_trace_expr())
}

fn physical_sensing_trace_expr() -> Expr {
    list(vec![
        sym("physical-sensing-trace"),
        list(vec![sym("id"), sym("a30-021-physical-sensing")]),
        list(vec![
            sym("fixture"),
            list(vec![sym("source"), sym("synthetic-sensor-stream")]),
            list(vec![sym("media"), sym("copied-no")]),
            list(vec![sym("device"), sym("live-device-none")]),
        ]),
        list(vec![
            sym("sensor-stream"),
            list(vec![sym("runner"), sym("fake-sensor-stream")]),
            list(vec![
                sym("frame"),
                sym("1"),
                sym("position"),
                sym("22"),
                sym("velocity"),
                sym("3"),
            ]),
            list(vec![
                sym("frame"),
                sym("2"),
                sym("position"),
                sym("24"),
                sym("velocity"),
                sym("2"),
            ]),
            list(vec![
                sym("frame"),
                sym("3"),
                sym("position"),
                sym("26"),
                sym("velocity"),
                sym("1"),
            ]),
        ]),
        list(vec![
            sym("temporal-average"),
            list(vec![sym("window"), sym("3")]),
            list(vec![sym("position"), sym("24")]),
            list(vec![sym("velocity"), sym("2")]),
        ]),
        list(vec![
            sym("controller"),
            list(vec![sym("kind"), sym("proportional")]),
            list(vec![sym("setpoint"), sym("30")]),
            list(vec![sym("gain"), sym("2")]),
            list(vec![sym("deadband"), sym("2")]),
            list(vec![sym("hysteresis"), sym("enabled")]),
        ]),
        list(vec![
            sym("control-output"),
            list(vec![sym("error"), sym("6")]),
            list(vec![sym("command"), sym("increase-12")]),
            list(vec![sym("clamped"), sym("no")]),
            list(vec![sym("next-state"), sym("approach-setpoint")]),
        ]),
        list(vec![sym("answer"), sym("increase-actuator-by-12")]),
        list(vec![
            sym("effect-ledger"),
            list(vec![
                sym("effect"),
                sym("read-fake-sensor-stream"),
                sym("deterministic"),
            ]),
            list(vec![
                sym("effect"),
                sym("average-window-three"),
                sym("pass"),
            ]),
            list(vec![sym("effect"), sym("apply-deadband"), sym("active")]),
            list(vec![
                sym("effect"),
                sym("emit-control-output"),
                sym("increase-12"),
            ]),
        ]),
    ])
}

fn list(items: Vec<Expr>) -> Expr {
    Expr::List(items)
}

fn sym(name: &str) -> Expr {
    Expr::Symbol(Symbol::new(name))
}

fn refs_from_args(cx: &mut Cx, args: Vec<Value>, context: &'static str) -> Result<Vec<Ref>> {
    args.iter()
        .map(|value| value_ref(cx, value, context))
        .collect()
}

fn continuation_ref(cx: &mut Cx, value: &Value) -> Result<Ref> {
    if let Some(continuation) = value.object().downcast_ref::<ContinuationValue>() {
        return Ok(continuation.continuation().clone());
    }
    value_ref(cx, value, "control continuation")
}

fn value_ref(cx: &mut Cx, value: &Value, context: &'static str) -> Result<Ref> {
    if let Some(result) = value.object().downcast_ref::<ControlResultValue>() {
        return Ok(result.reference().clone());
    }
    let expr = value.object().as_expr(cx)?;
    match expr {
        Expr::Symbol(symbol) => Ok(Ref::Symbol(symbol)),
        _ => Err(Error::TypeMismatch {
            expected: context,
            found: "non-ref value",
        }),
    }
}

fn optional_bool_arg(cx: &mut Cx, value: Option<&Value>) -> Result<bool> {
    let Some(value) = value else {
        return Ok(false);
    };
    match value.object().as_expr(cx)? {
        Expr::Bool(value) => Ok(value),
        _ => Err(Error::TypeMismatch {
            expected: "bool",
            found: "non-bool",
        }),
    }
}

fn control_result_value(cx: &mut Cx, reference: Ref) -> Result<Value> {
    cx.factory()
        .opaque(Arc::new(ControlResultValue::new(reference)))
}

fn arity_error(function: &'static str, expected: &'static str) -> Error {
    Error::Eval(format!("{function} expects {expected}"))
}
