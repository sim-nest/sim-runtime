use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Datum, Error, Export, Expr, Lib, LibManifest, LibTarget,
    Linker, LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};

use crate::WorldProduct;

/// CLI verb contributed by this loaded library.
pub const WORLD_VERB: &str = "world";

/// Loadable read-only `sim world` command library.
#[derive(Clone)]
pub struct WorldCommandLib {
    product: WorldProduct,
}

impl WorldCommandLib {
    /// Constructs the command library with the qualified bundled providers.
    pub fn new() -> std::result::Result<Self, crate::WorldError> {
        Ok(Self {
            product: WorldProduct::bundled()?,
        })
    }
}

impl Lib for WorldCommandLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("lib", "world-command"),
            version: Version(env!("CARGO_PKG_VERSION").into()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![Export::Function {
                symbol: sim_run_core::cli_main_entrypoint_symbol(WORLD_VERB),
                function_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.function_value(
            sim_run_core::cli_main_entrypoint_symbol(WORLD_VERB),
            cx.factory().opaque(Arc::new(WorldEntrypoint {
                product: self.product.clone(),
            }))?,
        )?;
        Ok(())
    }
}

#[derive(Clone)]
struct WorldEntrypoint {
    product: WorldProduct,
}

impl Object for WorldEntrypoint {
    fn display(&self, _: &mut Cx) -> Result<String> {
        Ok("cli/main/world".into())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for WorldEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for WorldEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        let argv = envelope_args(cx, args.values().first())?;
        let value = match argv.get(1).map(String::as_str) {
            Some("project") if argv.len() == 5 => self
                .product
                .project(&argv[2], &argv[3], Datum::String(argv[4].clone()), None)
                .map(|projection| projection.value),
            Some("diff") if argv.len() == 6 => self.product.diff(
                &argv[2],
                &argv[3],
                Datum::String(argv[4].clone()),
                Datum::String(argv[5].clone()),
            ),
            Some("why") if argv.len() == 4 => self.product.why(&argv[2], &argv[3]),
            _ => {
                return Err(Error::Eval(
                    "usage: sim world project KIND FACT VALUE | diff KIND FACT BEFORE AFTER | why CONCLUSION FACT".into(),
                ));
            }
        }
        .map_err(|error| Error::Eval(error.to_string()))?;
        println!("{}", render(&value));
        cx.factory().bool(true)
    }
}

fn envelope_args(cx: &mut Cx, envelope: Option<&Value>) -> Result<Vec<String>> {
    let envelope = envelope.ok_or_else(|| Error::Eval("missing world envelope".into()))?;
    let table = envelope
        .object()
        .as_table_impl()
        .ok_or_else(|| Error::Eval("world envelope is not a table".into()))?;
    let value = table.get(cx, Symbol::new("args"))?;
    let Expr::List(values) = value.object().as_expr(cx)? else {
        return Err(Error::Eval("world args are not a list".into()));
    };
    values
        .into_iter()
        .map(|value| match value {
            Expr::String(value) => Ok(value),
            _ => Err(Error::Eval("world arg is not a string".into())),
        })
        .collect()
}

fn render(value: &Datum) -> String {
    match value {
        Datum::Nil => "nil".to_owned(),
        Datum::Bool(value) => value.to_string(),
        Datum::String(value) => format!("\"{}\"", value.replace('"', "\\\"")),
        Datum::Symbol(value) => value.to_string(),
        Datum::Vector(values) => format!(
            "[{}]",
            values.iter().map(render).collect::<Vec<_>>().join(" ")
        ),
        Datum::List(values) => format!(
            "({})",
            values.iter().map(render).collect::<Vec<_>>().join(" ")
        ),
        Datum::Node { tag, fields } => {
            let fields = fields
                .iter()
                .map(|(name, value)| format!("({} {})", name, render(value)))
                .collect::<Vec<_>>()
                .join(" ");
            format!("#({tag} {fields})")
        }
        other => format!("{other:?}"),
    }
}
