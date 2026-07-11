use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Cx, Export, Lib, LibManifest, LibTarget, Linker, Result, Symbol, Version,
};

use crate::{
    claims::publish_control_organ_claims_for_lib,
    conditional::IfForm,
    ops::{ControlFunction, abort_symbol, capture_symbol, prompt_symbol, resume_symbol},
    policy::install_control_policy,
};

const CONTROL_LIB_ID: &str = "control";

/// The control organ as a loadable kernel [`Lib`].
///
/// Its manifest exports the `control/*` functions ([`control_exports`]) and its
/// `load` installs them as callables, registering this crate's control behavior
/// against the kernel's [`Lib`]/[`Linker`] contract.
pub struct ControlLib;

impl Lib for ControlLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: manifest_name(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: control_exports(),
        }
    }

    fn load(&self, cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        for function in [
            ControlFunction::prompt(),
            ControlFunction::capture(),
            ControlFunction::abort(),
            ControlFunction::resume(),
        ] {
            linker.function_value(function.symbol(), cx.factory().opaque(Arc::new(function))?)?;
        }
        linker.function_value(IfForm::symbol(), cx.factory().opaque(Arc::new(IfForm))?)?;
        Ok(())
    }
}

/// Installs the control organ into `cx`: loads [`ControlLib`] idempotently,
/// installs the default control policy, and publishes the organ's claims.
///
/// This is the first-reach entry point for the crate; everything else hangs off
/// the functions and policy it registers.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
///
/// use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};
/// use sim_lib_control::install_control_lib;
///
/// let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
/// install_control_lib(&mut cx).expect("install control organ");
/// // Idempotent: installing twice is a no-op on the second call.
/// install_control_lib(&mut cx).expect("reinstall is idempotent");
/// ```
pub fn install_control_lib(cx: &mut Cx) -> Result<()> {
    let lib_id = match sim_lib_core::install_once_id(cx, &ControlLib)? {
        Some(lib_id) => lib_id,
        None => sim_lib_core::installed_lib_id(cx, &ControlLib).expect("control lib is loaded"),
    };
    install_control_policy(cx);
    publish_control_organ_claims_for_lib(cx, lib_id)
}

/// Returns the lib's exported `control/*` functions as kernel [`Export`]s.
pub fn control_exports() -> Vec<Export> {
    [
        prompt_symbol(),
        capture_symbol(),
        abort_symbol(),
        resume_symbol(),
        IfForm::symbol(),
    ]
    .into_iter()
    .map(|symbol| Export::Function {
        symbol,
        function_id: None,
    })
    .collect()
}

/// Returns the `sim/control` manifest id under which this lib registers.
pub fn manifest_name() -> Symbol {
    Symbol::qualified("sim", CONTROL_LIB_ID)
}
