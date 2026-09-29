//! Loaded exposure of one exact host-bound ordinary process request.
//! conformance: selected calls require authority, load inertly and reject replacement input.

use crate::{ExecOptions, ProcResult, ProcessCancellation, ProcessPort, exec, exec_capability};
use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Error, Export, Expr, Lib, LibManifest, LibTarget, Linker,
    LoadCx, Object, ObjectCompat, Result, Symbol, Value, Version,
};
use std::sync::Arc;

/// One exact host-bound invocation exposed through the existing exec operation.
///
/// Loading grants no authority and starts no process. Both the callable and CLI
/// entry require `exec`; neither accepts replacement argv, bindings or budgets.
/// Results describe ordinary execution, never sandbox or checker qualification.
/// Each authorized call can execute again; this is not a durable once-only
/// acceptance. Cancellation remains cooperative through the injected port.
#[derive(Clone)]
pub struct ExecCommandLib {
    port: Arc<dyn ProcessPort>,
    argv: Vec<String>,
    options: ExecOptions,
    cancellation: ProcessCancellation,
}

impl ExecCommandLib {
    /// Binds a trusted host's exact request without granting or executing it.
    pub fn new(
        port: Arc<dyn ProcessPort>,
        argv: Vec<String>,
        options: ExecOptions,
        cancellation: ProcessCancellation,
    ) -> Result<Self> {
        crate::exec::checked_request(&argv, &options)?;
        Ok(Self {
            port,
            argv,
            options,
            cancellation,
        })
    }

    fn execute(&self, cx: &mut Cx) -> Result<ProcResult> {
        exec(
            cx,
            self.port.as_ref(),
            &self.argv,
            &self.options,
            &self.cancellation,
        )
    }
}

impl Lib for ExecCommandLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("lib", "operator-exec"),
            version: Version(env!("CARGO_PKG_VERSION").into()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: vec![exec_capability()],
            exports: [
                Symbol::qualified("exec", "selected"),
                sim_cli_core::cli_main_entrypoint_symbol("operator-exec"),
            ]
            .into_iter()
            .map(|symbol| Export::Function {
                symbol,
                function_id: None,
            })
            .collect(),
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        for cli in [false, true] {
            let symbol = if cli {
                sim_cli_core::cli_main_entrypoint_symbol("operator-exec")
            } else {
                Symbol::qualified("exec", "selected")
            };
            linker.function_value(
                symbol,
                cx.factory().opaque(Arc::new(Entrypoint {
                    command: self.clone(),
                    cli,
                }))?,
            )?;
        }
        Ok(())
    }
}

struct Entrypoint {
    command: ExecCommandLib,
    cli: bool,
}
impl Object for Entrypoint {
    fn display(&self, _: &mut Cx) -> Result<String> {
        Ok("host-bound exec".into())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
impl ObjectCompat for Entrypoint {
    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}
impl Callable for Entrypoint {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        cx.require(&exec_capability())?;
        if self.cli {
            let [envelope] = args.values() else {
                return Err(Error::Eval(
                    "operator exec requires one sealed boot envelope".into(),
                ));
            };
            let table = envelope
                .object()
                .as_table_impl()
                .ok_or_else(|| Error::Eval("operator exec envelope is not a table".into()))?;
            let value = table.get(cx, Symbol::new("args"))?;
            if !matches!(value.object().as_expr(cx)?, Expr::List(values)
                if matches!(values.as_slice(), [Expr::String(verb)] if verb == "operator-exec"))
            {
                return Err(Error::Eval(
                    "operator exec cannot replace its host-bound request".into(),
                ));
            }
        } else if !args.values().is_empty() {
            return Err(Error::Eval(
                "host-bound exec accepts no replacement arguments".into(),
            ));
        }
        let result = self.command.execute(cx)?;
        if !self.cli {
            return cx.factory().expr(result.to_constructor_expr());
        }
        print!("{}", result.stdout);
        eprint!("{}", result.stderr);
        if result.truncated {
            return Err(Error::HostError(
                "operator exec output truncated; result is incomplete".into(),
            ));
        }
        if !(0..=255).contains(&result.exit_code) {
            return Err(Error::HostError(
                "operator exec has no portable child exit status".into(),
            ));
        }
        cx.factory()
            .number_literal(Symbol::new("exit-code"), result.exit_code.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProcessAttempt, ProcessReceipt, ProcessRequest, ProgramRef, ProjectRootRef};
    use std::sync::Mutex;

    #[derive(Default)]
    struct Port(Mutex<Vec<ProcessRequest>>);
    impl ProcessPort for Port {
        fn run(&self, request: &ProcessRequest, _: &ProcessCancellation) -> ProcessAttempt {
            self.0.lock().unwrap().push(request.clone());
            ProcessAttempt::Completed {
                receipt: ProcessReceipt {
                    provider: "ordering-model".into(),
                    elapsed_mono_ns: 1,
                    result: ProcResult {
                        stdout: "model".into(),
                        stderr: String::new(),
                        exit_code: 101,
                        truncated: false,
                    },
                },
            }
        }
    }
    fn selected(port: Arc<Port>) -> ExecCommandLib {
        ExecCommandLib::new(
            port,
            vec!["exact argument".into()],
            ExecOptions::new(
                ProgramRef::new("p").unwrap(),
                ProjectRootRef::new("r").unwrap(),
                100,
                4096,
            ),
            ProcessCancellation::default(),
        )
        .unwrap()
    }

    #[test]
    fn bound_callable_checks_capability_and_replacement_before_dispatch() {
        let port = Arc::new(Port::default());
        let entry = Entrypoint {
            command: selected(port.clone()),
            cli: false,
        };
        let _manifest = entry.command.manifest();
        let mut cx = sim_kernel::testing::bare_cx();
        assert!(matches!(
            entry.call(&mut cx, Args::new(vec![])),
            Err(Error::CapabilityDenied { .. })
        ));
        cx.grant(exec_capability());
        let foreign = cx.factory().string("replacement".into()).unwrap();
        assert!(entry.call(&mut cx, Args::new(vec![foreign])).is_err());
        assert!(port.0.lock().unwrap().is_empty());
        let result = entry.call(&mut cx, Args::new(vec![])).unwrap();
        assert!(matches!(
            result.object().as_expr(&mut cx).unwrap(),
            Expr::Call { .. }
        ));
        let requests = port.0.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].argv[0].as_str(), "exact argument");
        assert_eq!(requests[0].environment.iter().count(), 0);
    }

    #[test]
    fn loading_selected_surface_is_inert_and_does_not_capture_general_exec() {
        let port = Arc::new(Port::default());
        let command = selected(port.clone());
        let mut cx = sim_kernel::testing::bare_cx();
        assert!(cx.load_lib(&command).is_err());
        assert!(port.0.lock().unwrap().is_empty());
        cx.grant(exec_capability());
        cx.load_lib(&command).unwrap();
        assert!(port.0.lock().unwrap().is_empty());
        assert!(
            cx.registry()
                .function_by_symbol(&Symbol::new("exec"))
                .is_none()
        );
        assert!(
            cx.registry()
                .function_by_symbol(&Symbol::qualified("exec", "selected"))
                .is_some()
        );
    }

    #[test]
    fn cli_envelope_cannot_replace_host_selection() {
        let port = Arc::new(Port::default());
        let entry = Entrypoint {
            command: selected(port.clone()),
            cli: true,
        };
        let mut cx = sim_kernel::testing::bare_cx();
        cx.grant(exec_capability());
        for words in [
            vec![],
            vec![Expr::String("foreign".into())],
            vec![
                Expr::String("operator-exec".into()),
                Expr::String("replacement".into()),
            ],
        ] {
            let list = cx.factory().expr(Expr::List(words)).unwrap();
            let envelope = cx
                .factory()
                .table(vec![(Symbol::new("args"), list)])
                .unwrap();
            assert!(entry.call(&mut cx, Args::new(vec![envelope])).is_err());
        }
        assert!(port.0.lock().unwrap().is_empty());
    }

    #[test]
    fn zero_budget_is_not_bindable() {
        let port = Arc::new(Port::default());
        for (time, output) in [(0, 1), (1, 0)] {
            assert!(
                ExecCommandLib::new(
                    port.clone(),
                    vec![],
                    ExecOptions::new(
                        ProgramRef::new("p").unwrap(),
                        ProjectRootRef::new("r").unwrap(),
                        time,
                        output,
                    ),
                    ProcessCancellation::default()
                )
                .is_err()
            );
        }
        assert!(port.0.lock().unwrap().is_empty());
    }
}
