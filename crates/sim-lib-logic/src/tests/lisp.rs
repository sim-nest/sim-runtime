use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use sim_codec_lisp::LispCodecLib;
use sim_kernel::{
    AbiVersion, CapabilitySet, Cx, DefaultFactory, EagerPolicy, Error, Expr, Lib, LibManifest,
    LibTarget, Linker, LoadCx, QuoteMode, ReadPolicy, Symbol, Version,
};
use sim_table_fs::{FsDir, table_fs_read_capability};

use crate::{
    LogicLib, install_logic_lib, logic_config_write_capability, logic_db_write_capability,
};

fn quote(expr: Expr) -> Expr {
    Expr::Quote {
        mode: QuoteMode::Quote,
        expr: Box::new(expr),
    }
}

fn fact(name: &str, args: Vec<Expr>) -> Expr {
    Expr::List(vec![
        Expr::Symbol(Symbol::new("fact")),
        Expr::List(
            std::iter::once(Expr::Symbol(Symbol::new(name)))
                .chain(args)
                .collect(),
        ),
    ])
}

fn logic_cx() -> Cx {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let lisp = LispCodecLib::new(cx.registry_mut().fresh_codec_id()).unwrap();
    cx.load_lib(&lisp).unwrap();
    install_logic_lib(&mut cx).unwrap();
    cx
}

fn test_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "sim-lib-logic-{name}-{}-{nanos}",
        std::process::id()
    ))
}

fn write_logic_fixture(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

struct ExportFsDirLib {
    lib_symbol: Symbol,
    export_symbol: Symbol,
    dir: FsDir,
}

impl ExportFsDirLib {
    fn new(export_symbol: Symbol, root: PathBuf) -> Self {
        Self {
            lib_symbol: Symbol::qualified("test", "logic-fixture-dir"),
            export_symbol,
            dir: FsDir::open(root).unwrap(),
        }
    }
}

impl Lib for ExportFsDirLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: self.lib_symbol.clone(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: vec![sim_kernel::Export::Value {
                symbol: self.export_symbol.clone(),
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> sim_kernel::Result<()> {
        linker.value(
            self.export_symbol.clone(),
            cx.factory().opaque(Arc::new(self.dir.clone()))?,
        )
    }
}

fn export_fs_dir(cx: &mut Cx, symbol: Symbol, root: PathBuf) {
    cx.load_lib(&ExportFsDirLib::new(symbol, root)).unwrap();
}

#[test]
fn install_logic_lib_registers_surface_and_assert_query_work() {
    let mut cx = logic_cx();
    cx.grant(logic_db_write_capability());
    let assert_fn = cx
        .resolve_function(&Symbol::qualified("logic", "assert!"))
        .unwrap();
    let query_fn = cx
        .resolve_function(&Symbol::qualified("logic", "query/all"))
        .unwrap();
    cx.call_exprs(
        assert_fn,
        vec![quote(Expr::List(vec![
            Expr::Symbol(Symbol::new("fact")),
            Expr::List(vec![
                Expr::Symbol(Symbol::new("parent")),
                Expr::Symbol(Symbol::new("alice")),
                Expr::Symbol(Symbol::new("bob")),
            ]),
        ]))],
    )
    .unwrap();
    let answers = cx
        .call_exprs(
            query_fn,
            vec![quote(Expr::List(vec![
                Expr::Symbol(Symbol::new("parent")),
                Expr::Symbol(Symbol::new("alice")),
                Expr::Local(Symbol::new("x")),
            ]))],
        )
        .unwrap();
    let expr = answers.object().as_expr(&mut cx).unwrap();
    assert!(matches!(expr, Expr::List(_)));
    let _ = LogicLib;
    let _ = ReadPolicy {
        trust: sim_kernel::TrustLevel::TrustedSource,
        capabilities: CapabilitySet::default(),
    };
}

#[test]
fn logic_consult_requires_fs_read_authority() {
    let mut cx = logic_cx();
    cx.grant(logic_db_write_capability());
    let root = test_root("consult-denied");
    write_logic_fixture(
        &root.join("rules").join("family.siml"),
        "((fact (parent alice bob)) (fact (parent alice carol)))",
    );
    let dir_symbol = Symbol::qualified("test", "rules-dir");
    export_fs_dir(&mut cx, dir_symbol.clone(), root);

    let consult_fn = cx
        .resolve_function(&Symbol::qualified("logic", "consult"))
        .unwrap();
    let err = cx
        .call_exprs(
            consult_fn,
            vec![
                Expr::Symbol(dir_symbol),
                Expr::String("rules/family".to_owned()),
            ],
        )
        .unwrap_err();

    assert!(matches!(
        err,
        Error::CapabilityDenied { capability } if capability == table_fs_read_capability()
    ));
}

#[test]
fn logic_consult_reads_relative_path_from_confined_dir() {
    let mut cx = logic_cx();
    cx.grant(logic_db_write_capability());
    cx.grant(table_fs_read_capability());
    let root = test_root("consult-allowed");
    write_logic_fixture(
        &root.join("rules").join("family.siml"),
        "((fact (parent alice bob)) (fact (parent alice carol)))",
    );
    let dir_symbol = Symbol::qualified("test", "rules-dir");
    export_fs_dir(&mut cx, dir_symbol.clone(), root);

    let consult_fn = cx
        .resolve_function(&Symbol::qualified("logic", "consult"))
        .unwrap();
    let consulted = cx
        .call_exprs(
            consult_fn,
            vec![
                Expr::Symbol(dir_symbol),
                Expr::String("rules/family".to_owned()),
            ],
        )
        .unwrap();
    assert_eq!(
        consulted.object().as_expr(&mut cx).unwrap(),
        Expr::String("2".to_owned())
    );

    let predicate_fn = cx
        .resolve_function(&Symbol::qualified("logic", "predicate?"))
        .unwrap();
    let exists = cx
        .call_exprs(
            predicate_fn,
            vec![quote(Expr::Symbol(Symbol::new("parent")))],
        )
        .unwrap();
    assert_eq!(exists.object().as_expr(&mut cx).unwrap(), Expr::Bool(true));
}

#[test]
fn logic_consult_bang_stays_pure_without_fs_read() {
    let mut cx = logic_cx();
    cx.grant(logic_db_write_capability());
    let consult_fn = cx
        .resolve_function(&Symbol::qualified("logic", "consult!"))
        .unwrap();
    let consulted = cx
        .call_exprs(
            consult_fn,
            vec![quote(Expr::List(vec![
                fact(
                    "parent",
                    vec![
                        Expr::Symbol(Symbol::new("alice")),
                        Expr::Symbol(Symbol::new("bob")),
                    ],
                ),
                fact(
                    "parent",
                    vec![
                        Expr::Symbol(Symbol::new("alice")),
                        Expr::Symbol(Symbol::new("carol")),
                    ],
                ),
            ]))],
        )
        .unwrap();
    assert_eq!(
        consulted.object().as_expr(&mut cx).unwrap(),
        Expr::String("2".to_owned())
    );
}

#[test]
fn logic_config_requires_write_capability() {
    let mut cx = logic_cx();
    let config_fn = cx
        .resolve_function(&Symbol::qualified("logic", "config"))
        .unwrap();
    let err = cx
        .call_exprs(
            config_fn,
            vec![
                Expr::Symbol(Symbol::new(":answer-limit")),
                Expr::String("4".to_owned()),
            ],
        )
        .unwrap_err();
    assert!(matches!(
        err,
        Error::CapabilityDenied { capability } if capability == logic_config_write_capability()
    ));
}
