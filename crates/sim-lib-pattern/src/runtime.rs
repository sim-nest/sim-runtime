//! The pattern organ as a loadable kernel [`Lib`].
//!
//! Registers the pattern special forms as callables against the kernel
//! [`Lib`]/[`Linker`] contract. The runtime entry point is the `match` form;
//! the crate's `Shape`/ADT machinery is the substrate it drives.

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Cx, Export, Lib, LibManifest, LibTarget, Linker, Result, Symbol, Version,
};

use crate::match_form::MatchForm;

const PATTERN_LIB_ID: &str = "pattern";

/// Returns the `sim/pattern` manifest id under which this lib registers.
pub fn manifest_name() -> Symbol {
    Symbol::qualified("sim", PATTERN_LIB_ID)
}

/// The pattern organ lib: installs the pattern special forms as callables.
pub struct PatternLib;

impl Lib for PatternLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: manifest_name(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: pattern_exports(),
        }
    }

    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.function_value(
            MatchForm::symbol(),
            cx.factory().opaque(Arc::new(MatchForm))?,
        )?;
        Ok(())
    }
}

/// Returns the lib's exported pattern forms as kernel [`Export`]s.
pub fn pattern_exports() -> Vec<Export> {
    vec![Export::Function {
        symbol: MatchForm::symbol(),
        function_id: None,
    }]
}

/// Installs the pattern organ into `cx` (idempotent).
pub fn install_pattern_lib(cx: &mut Cx) -> Result<()> {
    if cx.registry().lib(&manifest_name()).is_some() {
        return Ok(());
    }
    cx.load_lib(&PatternLib)?;
    Ok(())
}
