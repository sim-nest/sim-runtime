//! The binding organ as a loadable kernel [`Lib`].
//!
//! Registers the binding special forms as callables against the kernel
//! [`Lib`]/[`Linker`] contract. Today that is the `let` lexical binding form
//! (COOKBOOK_7 Category B); the crate's lexical/dynamic/mode machinery is the
//! substrate the forms build on.

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Cx, Export, Lib, LibManifest, LibTarget, Linker, Result, Symbol, Version,
};

use crate::let_form::LetForm;

const BINDING_LIB_ID: &str = "binding";

/// Returns the `sim/binding` manifest id under which this lib registers.
pub fn manifest_name() -> Symbol {
    Symbol::qualified("sim", BINDING_LIB_ID)
}

/// The binding organ lib: installs the binding special forms as callables.
pub struct BindingLib;

impl Lib for BindingLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: manifest_name(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: binding_exports(),
        }
    }

    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.function_value(LetForm::symbol(), cx.factory().opaque(Arc::new(LetForm))?)?;
        Ok(())
    }
}

/// Returns the lib's exported binding forms as kernel [`Export`]s.
pub fn binding_exports() -> Vec<Export> {
    vec![Export::Function {
        symbol: LetForm::symbol(),
        function_id: None,
    }]
}

/// Installs the binding organ into `cx` (idempotent): loads [`BindingLib`] only
/// when it is not already registered.
pub fn install_binding_lib(cx: &mut Cx) -> Result<()> {
    if cx.registry().lib(&manifest_name()).is_some() {
        return Ok(());
    }
    cx.load_lib(&BindingLib)?;
    Ok(())
}
