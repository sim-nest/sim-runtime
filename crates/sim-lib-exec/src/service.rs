//! Loaded entrypoints for separate administrative, worker and payload owners.

use sim_cli_core::cli_main_entrypoint_symbol;
use sim_kernel::{
    AbiVersion, Args, Callable, CapabilityName, Cx, Error, Export, Expr, Lib, LibManifest,
    LibTarget, Linker, LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};
use std::sync::Arc;

/// Boot-configured native execution service beneath a loaded SIM entrypoint.
///
/// All resources, command allowlists, inherited channels and gate ownership are
/// supplied at boot. Transport messages cannot replace this configuration.
/// Worker services retain accepted execution independently of client lifetime;
/// payload-entry services admit no untrusted work before the native gate opens.
/// The service owns its execution loop and must not retain or create a `Cx`.
pub trait ExecServicePort: Send + Sync {
    /// Runs the exact boot-selected service; errors preserve unresolved custody.
    fn run(&self) -> Result<()>;
}

#[derive(Clone, Copy)]
enum Entry {
    Administrative,
    Worker,
    Payload,
    ControllerSender,
    ServiceStartSender,
}

impl Entry {
    fn verb(self) -> &'static str {
        match self {
            Self::Administrative => "local-check-administrator",
            Self::Worker => "local-check-worker",
            Self::Payload => "local-check-payload",
            Self::ControllerSender => "local-check-controller",
            Self::ServiceStartSender => "local-check-service-start",
        }
    }

    fn capability(self) -> CapabilityName {
        CapabilityName::new(match self {
            Self::Administrative => "exec/local-check-administrator",
            Self::Worker => "exec/local-check-worker",
            Self::Payload => "exec/local-check-payload",
            Self::ControllerSender => "exec/local-check-controller-send",
            Self::ServiceStartSender => "exec/local-check-service-start-send",
        })
    }
}

/// One closed, host-registered execution service profile.
///
/// Each instance exports only its selected entrypoint. The existing bootloader
/// supplies the runtime; native behavior and process/resource lifetime remain
/// in the injected capsule service. Loading the library alone starts no service.
#[derive(Clone)]
pub struct ExecServiceLib {
    entry: Entry,
    service: Arc<dyn ExecServicePort>,
}

impl ExecServiceLib {
    /// Returns this concrete profile's one required capability name.
    /// This is data, not a grant. Only the admitting host may supply authority;
    /// an arbitrary library manifest cannot select a grant through this method.
    pub fn required_capability(&self) -> CapabilityName {
        self.entry.capability()
    }

    /// Selects the independently installed surviving resource-owner profile.
    ///
    /// This declares only a loaded entry and its distinct capability. It cannot
    /// elevate identity, authenticate installation, allocate a reservation or
    /// grant execution. The host must admit the concrete administrative service
    /// before supplying its capability; worker and payload grants never suffice.
    /// Resource, cancellation and recovery operations remain in that service,
    /// using the existing journal and native owners rather than another executor.
    pub fn administrative(service: Arc<dyn ExecServicePort>) -> Self {
        Self {
            entry: Entry::Administrative,
            service,
        }
    }

    /// Selects the surviving exact-command worker profile.
    pub fn worker(service: Arc<dyn ExecServicePort>) -> Self {
        Self {
            entry: Entry::Worker,
            service,
        }
    }

    /// Selects trusted post-sandbox entry for one sealed native payload.
    pub fn payload(service: Arc<dyn ExecServicePort>) -> Self {
        Self {
            entry: Entry::Payload,
            service,
        }
    }

    /// Selects the closed non-administrative, send-only control profile.
    /// The admitting host selects inert intent and one bounded transport attempt.
    /// This role must never receive a response or obtain execution authority.
    /// Service success means transport action only, not accepted cancellation,
    /// stopped resources, an operation outcome or checker qualification.
    pub fn controller_sender(service: Arc<dyn ExecServicePort>) -> Self {
        Self {
            entry: Entry::ControllerSender,
            service,
        }
    }

    /// Selects the closed non-administrative requester for one fixed service
    /// activation. This distinct entrypoint prevents a controller sender from
    /// being relabelled by its envelope while retaining another exported verb.
    pub fn service_start_sender(service: Arc<dyn ExecServicePort>) -> Self {
        Self {
            entry: Entry::ServiceStartSender,
            service,
        }
    }
}

impl Lib for ExecServiceLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("lib", self.entry.verb()),
            version: Version(env!("CARGO_PKG_VERSION").into()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: vec![self.entry.capability()],
            exports: vec![Export::Function {
                symbol: cli_main_entrypoint_symbol(self.entry.verb()),
                function_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.function_value(
            cli_main_entrypoint_symbol(self.entry.verb()),
            cx.factory()
                .opaque(Arc::new(ServiceEntrypoint(self.clone())))?,
        )?;
        Ok(())
    }
}

struct ServiceEntrypoint(ExecServiceLib);

impl Object for ServiceEntrypoint {
    fn display(&self, _: &mut Cx) -> Result<String> {
        Ok(format!("cli/main/{}", self.0.entry.verb()))
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ServiceEntrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

impl Callable for ServiceEntrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        cx.require(&self.0.entry.capability())?;
        let [envelope] = args.values() else {
            return Err(Error::Eval(
                "execution service requires one boot envelope".into(),
            ));
        };
        let table = envelope
            .object()
            .as_table_impl()
            .ok_or_else(|| Error::Eval("execution service boot envelope is not a table".into()))?;
        let value = table.get(cx, Symbol::new("args"))?;
        let Expr::List(arguments) = value.object().as_expr(cx)? else {
            return Err(Error::Eval(
                "execution service arguments are not a list".into(),
            ));
        };
        validate_arguments(self.0.entry, &arguments)?;
        // Native service loops receive no borrowed Cx or runtime lock. They own
        // accepted work and cancellation routing beyond each client connection.
        self.0.service.run()?;
        cx.factory().bool(true)
    }
}

fn validate_arguments(entry: Entry, arguments: &[Expr]) -> Result<()> {
    if matches!(arguments, [Expr::String(verb)] if verb == entry.verb()) {
        Ok(())
    } else {
        Err(Error::Eval(
            "execution service requires its exact boot-selected verb and no extra arguments".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Service(AtomicUsize);
    impl ExecServicePort for Service {
        fn run(&self) -> Result<()> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[test]
    fn profile_exports_only_its_boot_selected_service_without_running_it() {
        let service = Arc::new(Service::default());
        for lib in [
            ExecServiceLib::administrative(service.clone()),
            ExecServiceLib::worker(service.clone()),
            ExecServiceLib::payload(service.clone()),
            ExecServiceLib::controller_sender(service.clone()),
            ExecServiceLib::service_start_sender(service.clone()),
        ] {
            let manifest = lib.manifest();
            assert_eq!(manifest.exports.len(), 1);
            assert!(
                matches!(&manifest.exports[0], Export::Function { symbol, .. }
                if symbol == &cli_main_entrypoint_symbol(lib.entry.verb()))
            );
            assert_eq!(manifest.capabilities, vec![lib.entry.capability()]);
        }
        assert_eq!(service.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn service_arguments_cannot_replace_the_boot_selected_native_configuration() {
        for entry in [
            Entry::Administrative,
            Entry::Worker,
            Entry::Payload,
            Entry::ControllerSender,
            Entry::ServiceStartSender,
        ] {
            assert!(validate_arguments(entry, &[Expr::String(entry.verb().into())]).is_ok());
            for args in [
                vec![],
                vec![Expr::String("foreign".into())],
                vec![
                    Expr::String(entry.verb().into()),
                    Expr::String("--program=foreign".into()),
                ],
            ] {
                assert!(validate_arguments(entry, &args).is_err());
            }
        }
    }

    #[test]
    fn capability_and_exact_envelope_precede_service_dispatch() {
        for selected in [
            Entry::Administrative,
            Entry::Worker,
            Entry::Payload,
            Entry::ControllerSender,
            Entry::ServiceStartSender,
        ] {
            check_selected_capability(selected);
        }
    }

    fn check_selected_capability(selected: Entry) {
        let service = Arc::new(Service::default());
        let entry = ServiceEntrypoint(ExecServiceLib {
            entry: selected,
            service: service.clone(),
        });
        let mut cx = sim_kernel::testing::bare_cx();
        assert!(matches!(
            entry.call(&mut cx, Args::new(vec![])),
            Err(Error::CapabilityDenied { .. })
        ));
        for foreign in [
            Entry::Administrative,
            Entry::Worker,
            Entry::Payload,
            Entry::ControllerSender,
            Entry::ServiceStartSender,
        ] {
            if foreign.capability() != selected.capability() {
                cx.grant(foreign.capability());
            }
        }
        assert!(matches!(
            entry.call(&mut cx, Args::new(vec![])),
            Err(Error::CapabilityDenied { .. })
        ));
        assert_eq!(service.0.load(Ordering::SeqCst), 0);
        cx.grant(selected.capability());
        assert!(entry.call(&mut cx, Args::new(vec![])).is_err());
        for arguments in [
            vec![
                Expr::String(selected.verb().into()),
                Expr::String("--replace-policy".into()),
            ],
            vec![Expr::String(selected.verb().into())],
        ] {
            let valid = arguments.len() == 1;
            let list = cx.factory().expr(Expr::List(arguments)).unwrap();
            let envelope = cx
                .factory()
                .table(vec![(Symbol::new("args"), list)])
                .unwrap();
            assert_eq!(
                entry.call(&mut cx, Args::new(vec![envelope])).is_ok(),
                valid
            );
        }
        assert_eq!(service.0.load(Ordering::SeqCst), 1);
    }
}
