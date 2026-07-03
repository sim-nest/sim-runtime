//! Surface-pack card libs and idempotent install.
//!
//! ~92 crates hand-write an `impl Lib` that exports a set of value cards
//! (tables with fields like `symbol`, `layer`, `kind`, `role`, ...) and ~49
//! guard install with `registry().lib(&id).is_some()`. This is their shared
//! substrate: declare the cards as data ([`SurfacePackSpec`]) and install once
//! with [`install_once`].
//!
//! # How to write a SIM lib
//!
//! A host-registered SIM lib should keep its crate entrypoint thin, put the
//! implementation in a focused runtime module, and expose one public install
//! function named `install_<crate>_lib` after stripping the `sim-lib-` prefix and
//! replacing hyphens with underscores. The install function should call
//! [`install_once`] rather than hand-checking `registry().lib(...)`, unless the
//! crate is a documented aggregate or lower-layer exception in
//! `scripts/check-libs.sh`.
//!
//! `Lib::manifest` should use `Version(env!("CARGO_PKG_VERSION").to_owned())`,
//! `LibTarget::HostRegistered`, explicit `Export` records for every registered
//! class/function/value/codec/number-domain surface, and only the capabilities
//! the lib itself requires at load time. `Lib::load` owns the actual linker
//! registration and should avoid hidden global mutation beyond the registered
//! runtime surface.
//!
//! If the lib publishes value cards, prefer [`SurfacePackLib`] and
//! [`SurfacePackSpec`] over hand-written card registration. If the lib ships
//! recipes, keep the pure recipe data in `sim-cookbook` inputs and register the
//! runtime projection from the higher-level lib. If it introduces object values,
//! either derive/provide citizen read constructors or add an explicit
//! `#[non_citizen]` exemption that names the descriptor strategy.

use sim_kernel::{
    AbiVersion, Cx, Dependency, Export, Expr, Lib, LibId, LibManifest, LibTarget, Linker, LoadCx,
    Result, Symbol, Value, Version,
};

/// A typed card field value.
pub enum SurfaceField {
    /// A symbol value.
    Symbol(Symbol),
    /// A string value.
    Str(String),
    /// A list-of-symbols value (built as a list of symbol values).
    Symbols(Vec<Symbol>),
    /// A list-of-strings value (built as a list of string values).
    Strs(Vec<String>),
    /// A boolean value.
    Bool(bool),
    /// An unsigned-integer (i64-domain number) value.
    U64(u64),
    /// An arbitrary expression value.
    Expr(Expr),
}

/// One exported value card: its symbol and its table fields.
pub struct SurfaceValueSpec {
    /// The card's export symbol.
    pub symbol: Symbol,
    /// The card's table fields, in order.
    pub fields: Vec<(Symbol, SurfaceField)>,
}

/// A pack of value cards exported by one host-registered lib.
pub struct SurfacePackSpec {
    /// The lib id.
    pub lib_id: Symbol,
    /// The cards.
    pub values: Vec<SurfaceValueSpec>,
}

/// A host-registered lib built from a [`SurfacePackSpec`].
pub struct SurfacePackLib {
    /// The pack specification.
    pub spec: SurfacePackSpec,
}

impl Lib for SurfacePackLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: self.spec.lib_id.clone(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::<Dependency>::new(),
            capabilities: Vec::new(),
            exports: self
                .spec
                .values
                .iter()
                .map(|value| Export::Value {
                    symbol: value.symbol.clone(),
                })
                .collect(),
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        for value in &self.spec.values {
            let card = build_card(cx, &value.fields)?;
            linker.value(value.symbol.clone(), card)?;
        }
        Ok(())
    }
}

fn build_card(cx: &mut LoadCx, fields: &[(Symbol, SurfaceField)]) -> Result<Value> {
    let mut entries = Vec::with_capacity(fields.len());
    for (key, field) in fields {
        let value = match field {
            SurfaceField::Symbol(symbol) => cx.factory().symbol(symbol.clone())?,
            SurfaceField::Str(text) => cx.factory().string(text.clone())?,
            SurfaceField::Bool(flag) => cx.factory().bool(*flag)?,
            SurfaceField::Symbols(symbols) => {
                let items = symbols
                    .iter()
                    .map(|symbol| cx.factory().symbol(symbol.clone()))
                    .collect::<Result<Vec<_>>>()?;
                cx.factory().list(items)?
            }
            SurfaceField::Strs(texts) => {
                let items = texts
                    .iter()
                    .map(|text| cx.factory().string(text.clone()))
                    .collect::<Result<Vec<_>>>()?;
                cx.factory().list(items)?
            }
            other => cx.factory().expr(field_to_expr(other))?,
        };
        entries.push((key.clone(), value));
    }
    cx.factory().table(entries)
}

/// Build the browse card map (an `Expr`) for one value spec -- the shared core
/// of the many hand-written `*_card_expr` builders. This is the `Expr` (data)
/// counterpart of the `Value` card that [`SurfacePackLib`] registers.
pub fn card_expr(spec: &SurfaceValueSpec) -> Expr {
    Expr::Map(
        spec.fields
            .iter()
            .map(|(key, field)| (Expr::Symbol(key.clone()), field_to_expr(field)))
            .collect(),
    )
}

fn field_to_expr(field: &SurfaceField) -> Expr {
    match field {
        SurfaceField::Symbol(symbol) => Expr::Symbol(symbol.clone()),
        SurfaceField::Str(text) => Expr::String(text.clone()),
        SurfaceField::Bool(flag) => Expr::Bool(*flag),
        SurfaceField::U64(number) => sim_value::build::uint(*number),
        SurfaceField::Symbols(symbols) => Expr::List(
            symbols
                .iter()
                .map(|symbol| Expr::Symbol(symbol.clone()))
                .collect(),
        ),
        SurfaceField::Strs(texts) => Expr::List(
            texts
                .iter()
                .map(|text| Expr::String(text.clone()))
                .collect(),
        ),
        SurfaceField::Expr(expr) => expr.clone(),
    }
}

/// Install `lib` only if its id is not already registered. Returns `true` if it
/// was loaded, `false` if it was already present. Replaces the
/// `registry().lib(&id).is_some()` early-return guard.
pub fn install_once(cx: &mut Cx, lib: &impl Lib) -> Result<bool> {
    install_once_id(cx, lib).map(|id| id.is_some())
}

/// Install `lib` only if absent, returning the newly loaded id when it loads.
pub fn install_once_id(cx: &mut Cx, lib: &impl Lib) -> Result<Option<LibId>> {
    let id = lib.manifest().id.clone();
    if cx.registry().lib(&id).is_some() {
        return Ok(None);
    }
    cx.load_lib(lib).map(Some)
}

/// Return the loaded id for `lib`, if it is already registered.
pub fn installed_lib_id(cx: &Cx, lib: &impl Lib) -> Option<LibId> {
    let id = lib.manifest().id;
    cx.registry().lib(&id).map(|loaded| loaded.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack() -> SurfacePackLib {
        SurfacePackLib {
            spec: SurfacePackSpec {
                lib_id: Symbol::new("demo-pack"),
                values: vec![
                    SurfaceValueSpec {
                        symbol: Symbol::qualified("demo", "Alpha"),
                        fields: vec![
                            (
                                Symbol::new("symbol"),
                                SurfaceField::Symbol(Symbol::qualified("demo", "Alpha")),
                            ),
                            (Symbol::new("layer"), SurfaceField::Str("demo".to_owned())),
                            (Symbol::new("lossless"), SurfaceField::Bool(true)),
                            (Symbol::new("rank"), SurfaceField::U64(3)),
                            (
                                Symbol::new("tags"),
                                SurfaceField::Strs(vec!["a".to_owned(), "b".to_owned()]),
                            ),
                        ],
                    },
                    SurfaceValueSpec {
                        symbol: Symbol::qualified("demo", "Beta"),
                        fields: vec![(
                            Symbol::new("symbol"),
                            SurfaceField::Symbol(Symbol::qualified("demo", "Beta")),
                        )],
                    },
                ],
            },
        }
    }

    #[test]
    fn manifest_exports_one_value_per_card() {
        let manifest = pack().manifest();
        assert_eq!(manifest.id, Symbol::new("demo-pack"));
        let symbols: Vec<String> = manifest
            .exports
            .iter()
            .filter_map(|export| match export {
                Export::Value { symbol } => Some(symbol.to_string()),
                _ => None,
            })
            .collect();
        assert_eq!(
            symbols,
            vec!["demo/Alpha".to_owned(), "demo/Beta".to_owned()]
        );
    }

    #[test]
    fn card_expr_renders_every_field_kind() {
        let spec = &pack().spec.values[0];
        let Expr::Map(entries) = card_expr(spec) else {
            panic!("card_expr must build a map");
        };
        assert_eq!(entries.len(), 5);
        assert_eq!(
            entries[0],
            (
                Expr::Symbol(Symbol::new("symbol")),
                Expr::Symbol(Symbol::qualified("demo", "Alpha"))
            )
        );
        assert_eq!(
            entries[1],
            (
                Expr::Symbol(Symbol::new("layer")),
                Expr::String("demo".to_owned())
            )
        );
        assert_eq!(
            entries[2],
            (Expr::Symbol(Symbol::new("lossless")), Expr::Bool(true))
        );
        assert_eq!(
            entries[3],
            (Expr::Symbol(Symbol::new("rank")), sim_value::build::uint(3))
        );
        assert_eq!(
            entries[4].1,
            Expr::List(vec![
                Expr::String("a".to_owned()),
                Expr::String("b".to_owned())
            ])
        );
    }

    #[test]
    fn install_once_is_idempotent_and_loads_every_field_kind() {
        let mut cx = sim_test_support::core_cx();
        assert!(
            install_once(&mut cx, &pack()).unwrap(),
            "first install loads"
        );
        assert!(
            !install_once(&mut cx, &pack()).unwrap(),
            "second install is a no-op"
        );
    }
}
