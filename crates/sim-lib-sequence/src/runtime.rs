//! The sequence organ as a loadable kernel [`Lib`].
//!
//! Registers the higher-order sequence operations `seq/map`, `seq/filter`, and
//! `seq/fold` as callables (COOKBOOK_7 Category B). These are ordinary functions
//! -- both the applied function and the collection are evaluated before the op
//! runs -- but they are eval-policy organs in that they APPLY a function value
//! over every element, driving the evaluator once per element via
//! [`Cx::call_value`].

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, ClassRef, Cx, Error, Export, Lib, LibManifest, LibTarget, Linker,
    Object, ObjectCompat, Result, Symbol, Value, Version, force_list_to_vec,
};

const SEQUENCE_LIB_ID: &str = "sequence";

/// Returns the `sim/sequence` manifest id under which this lib registers.
pub fn manifest_name() -> Symbol {
    Symbol::qualified("sim", SEQUENCE_LIB_ID)
}

/// One higher-order sequence operation.
#[derive(Clone, Copy)]
pub enum SeqOp {
    /// `(seq/map f list)` -> the list of `f(x)` for each `x`.
    Map,
    /// `(seq/filter pred list)` -> the elements for which `pred(x)` is truthy.
    Filter,
    /// `(seq/fold f init list)` -> the left fold `f(... f(f(init, x0), x1) ..., xn)`.
    Fold,
}

impl SeqOp {
    /// All sequence operations, in registration order.
    pub const ALL: [SeqOp; 3] = [SeqOp::Map, SeqOp::Filter, SeqOp::Fold];

    /// The `seq/*` symbol this operation registers under.
    pub fn symbol(self) -> Symbol {
        let name = match self {
            SeqOp::Map => "map",
            SeqOp::Filter => "filter",
            SeqOp::Fold => "fold",
        };
        Symbol::qualified("seq", name)
    }

    fn arity(self) -> usize {
        match self {
            SeqOp::Map | SeqOp::Filter => 2,
            SeqOp::Fold => 3,
        }
    }

    /// Extracts the elements of `value`, erroring when it is not a list.
    fn elements(cx: &mut Cx, value: &Value, context: &'static str) -> Result<Vec<Value>> {
        let object = value.object();
        let Some(list) = object.as_list() else {
            return Err(Error::TypeMismatch {
                expected: "list",
                found: context,
            });
        };
        force_list_to_vec(cx, list, context)
    }

    fn run(self, cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
        if args.len() != self.arity() {
            return Err(Error::Eval(format!(
                "{} expects {} argument(s), got {}",
                self.symbol(),
                self.arity(),
                args.len()
            )));
        }
        match self {
            SeqOp::Map => {
                let items = Self::elements(cx, &args[1], "seq/map list")?;
                let func = args[0].clone();
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(cx.call_value(func.clone(), Args::new(vec![item]))?);
                }
                cx.factory().list(out)
            }
            SeqOp::Filter => {
                let items = Self::elements(cx, &args[1], "seq/filter list")?;
                let pred = args[0].clone();
                let mut out = Vec::new();
                for item in items {
                    let keep = cx
                        .call_value(pred.clone(), Args::new(vec![item.clone()]))?
                        .object()
                        .truth(cx)?;
                    if keep {
                        out.push(item);
                    }
                }
                cx.factory().list(out)
            }
            SeqOp::Fold => {
                let items = Self::elements(cx, &args[2], "seq/fold list")?;
                let func = args[0].clone();
                let mut acc = args[1].clone();
                for item in items {
                    acc = cx.call_value(func.clone(), Args::new(vec![acc, item]))?;
                }
                Ok(acc)
            }
        }
    }
}

/// A callable runtime object exposing one [`SeqOp`].
#[derive(Clone)]
pub struct SequenceFunction {
    op: SeqOp,
}

impl SequenceFunction {
    /// Builds the callable for `op`.
    pub fn new(op: SeqOp) -> Self {
        Self { op }
    }
}

impl Object for SequenceFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<function {}>", self.op.symbol()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for SequenceFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.resolve_class(&Symbol::qualified("core", "Function"))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for SequenceFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        self.op.run(cx, args.into_vec())
    }
}

/// The sequence organ lib: installs `seq/map|filter|fold` as callables.
pub struct SequenceLib;

impl Lib for SequenceLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: manifest_name(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: sequence_exports(),
        }
    }

    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        for op in SeqOp::ALL {
            let function = SequenceFunction::new(op);
            linker.function_value(op.symbol(), cx.factory().opaque(Arc::new(function))?)?;
        }
        Ok(())
    }
}

/// Returns the lib's exported `seq/*` functions as kernel [`Export`]s.
pub fn sequence_exports() -> Vec<Export> {
    SeqOp::ALL
        .into_iter()
        .map(|op| Export::Function {
            symbol: op.symbol(),
            function_id: None,
        })
        .collect()
}

/// Installs the sequence organ into `cx` (idempotent).
pub fn install_sequence_lib(cx: &mut Cx) -> Result<()> {
    if cx.registry().lib(&manifest_name()).is_some() {
        return Ok(());
    }
    cx.load_lib(&SequenceLib)?;
    Ok(())
}
