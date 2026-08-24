//! Loadable command product and pure projections over a study evidence graph.

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, Callable, Cx, Error, Export, Lib, LibManifest, LibTarget, Linker, LoadCx,
    Object, ObjectCompat, Result, Symbol, Value, Version,
};

/// Every public operation over the one study evidence graph.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StudyVerb {
    Plan,
    Seal,
    Run,
    Resume,
    Cancel,
    Status,
    Show,
    Verify,
    Report,
    Decide,
    Select,
    Export,
}

impl StudyVerb {
    /// Parses the stable command vocabulary.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "plan" => Self::Plan,
            "seal" => Self::Seal,
            "run" => Self::Run,
            "resume" => Self::Resume,
            "cancel" => Self::Cancel,
            "status" => Self::Status,
            "show" => Self::Show,
            "verify" => Self::Verify,
            "report" => Self::Report,
            "decide" => Self::Decide,
            "select" => Self::Select,
            "export" => Self::Export,
            _ => return None,
        })
    }

    /// Whether the operation may invoke an executor.
    pub const fn may_execute(self) -> bool {
        matches!(self, Self::Run | Self::Resume)
    }

    /// Whether the operation may append evidence. Planning is write-capable
    /// only with an explicit confirmation flag; sealing and cancellation append
    /// authority/control facts but never execute candidate work.
    pub const fn may_write(self, confirmed: bool) -> bool {
        matches!(self, Self::Seal | Self::Run | Self::Resume | Self::Cancel)
            || (matches!(self, Self::Plan) && confirmed)
    }
}

/// Stable process contract. Values deliberately leave room for future detailed
/// refusal codes without conflating a valid incomplete study with corruption.
#[repr(i32)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StudyExit {
    Complete = 0,
    Incomplete = 10,
    Invalid = 20,
    Refused = 30,
    Signalled = 130,
}

/// Parsed invocation. All observational commands are rejected if a caller
/// attempts to attach write confirmation to them.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StudyCommand {
    pub verb: StudyVerb,
    pub graph: String,
    pub confirmed: bool,
}

impl StudyCommand {
    pub fn parse(args: &[String]) -> std::result::Result<Self, String> {
        let verb = args.first().and_then(|v| StudyVerb::parse(v)).ok_or_else(|| {
            "expected study plan|seal|run|resume|cancel|status|show|verify|report|decide|select|export".to_owned()
        })?;
        let mut graph = None;
        let mut confirmed = false;
        let mut index = 1;
        while index < args.len() {
            match args[index].as_str() {
                "--graph" => {
                    index += 1;
                    graph = args.get(index).cloned();
                }
                "--confirm" => confirmed = true,
                other => return Err(format!("unknown study argument: {other}")),
            }
            index += 1;
        }
        if confirmed && !matches!(verb, StudyVerb::Plan) {
            return Err("--confirm is valid only for study plan".into());
        }
        Ok(Self {
            verb,
            graph: graph.ok_or_else(|| "study requires --graph PATH".to_owned())?,
            confirmed,
        })
    }
}

/// Pure command classification used by host adapters and fixture executors.
pub fn classify(command: &StudyCommand, complete: bool) -> StudyExit {
    if command.verb.may_execute() {
        StudyExit::Incomplete
    } else if complete {
        StudyExit::Complete
    } else {
        StudyExit::Incomplete
    }
}

/// Host-loadable `cli/main/study` product.
#[derive(Clone, Default)]
pub struct StudyLib;

impl Lib for StudyLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: Symbol::qualified("lib", "study"),
            version: Version(env!("CARGO_PKG_VERSION").into()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: vec![],
            capabilities: vec![],
            exports: vec![Export::Function {
                symbol: entrypoint_symbol(),
                function_id: None,
            }],
        }
    }

    fn load(&self, cx: &mut LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        linker.function_value(
            entrypoint_symbol(),
            cx.factory().opaque(Arc::new(Entrypoint))?,
        )?;
        Ok(())
    }
}

pub fn entrypoint_symbol() -> Symbol {
    Symbol::qualified("cli", "main/study")
}

struct Entrypoint;
impl Object for Entrypoint {
    fn display(&self, _: &mut Cx) -> Result<String> {
        Ok("cli/main/study".into())
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
        let envelope = args
            .values()
            .first()
            .ok_or_else(|| Error::Eval("missing study envelope".into()))?;
        let table = envelope
            .object()
            .as_table_impl()
            .ok_or_else(|| Error::Eval("study envelope is not a table".into()))?;
        let sim_kernel::Expr::List(items) =
            table.get(cx, Symbol::new("args"))?.object().as_expr(cx)?
        else {
            return Err(Error::Eval("study args are not a list".into()));
        };
        let argv = items
            .into_iter()
            .filter_map(|item| match item {
                sim_kernel::Expr::String(s) => Some(s),
                _ => None,
            })
            .collect::<Vec<_>>();
        let command = StudyCommand::parse(&argv[1..]).map_err(Error::Eval)?;
        println!(
            "study {:?} graph={} effect={} confirmed={}",
            command.verb,
            command.graph,
            command.verb.may_execute(),
            command.confirmed
        );
        let exit = classify(&command, false) as i32;
        cx.factory()
            .number_literal(Symbol::new("exit-code"), exit.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        decision::{
            AttributedResourceEvent, DecisionSpec, DimensionSpec, Direction, ResourceCause,
            StalenessInputs, SubjectEvidence, decide, select, summarize_spend,
        },
        design::{
            ConfirmationDesign, DesignCell, DesignSeal, EliminationPolicy, ResolutionPolicy,
            ScreeningDecision, SequentialScreen, SmokeDiagnosis, seal_confirmation, smoke,
        },
    };
    use sha2::{Digest, Sha256};
    use sim_kernel::ContentId;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn complete_vocabulary_and_effect_boundary_are_closed() {
        let all = [
            "plan", "seal", "run", "resume", "cancel", "status", "show", "verify", "report",
            "decide", "select", "export",
        ];
        assert!(all.into_iter().all(|verb| StudyVerb::parse(verb).is_some()));
        for verb in [
            StudyVerb::Status,
            StudyVerb::Show,
            StudyVerb::Verify,
            StudyVerb::Report,
            StudyVerb::Decide,
            StudyVerb::Select,
            StudyVerb::Export,
        ] {
            assert!(!verb.may_execute());
            assert!(!verb.may_write(false));
        }
        assert!(StudyVerb::Run.may_execute() && StudyVerb::Resume.may_execute());
        assert!(!StudyVerb::Plan.may_write(false) && StudyVerb::Plan.may_write(true));
    }

    #[test]
    fn exit_contract_is_disjoint() {
        assert_eq!(
            [
                StudyExit::Complete as i32,
                StudyExit::Incomplete as i32,
                StudyExit::Invalid as i32,
                StudyExit::Refused as i32,
                StudyExit::Signalled as i32
            ],
            [0, 10, 20, 30, 130]
        );
    }

    #[test]
    fn exports_loadable_study_entrypoint() {
        assert!(Lib::manifest(&StudyLib).exports.iter().any(
            |e| matches!(e, Export::Function { symbol, .. } if symbol == &entrypoint_symbol())
        ));
    }

    trait FixtureExecutor {
        fn delivered(&self, input: u64) -> u64;
    }
    struct Iterative;
    struct ClosedForm;
    impl FixtureExecutor for Iterative {
        fn delivered(&self, input: u64) -> u64 {
            (1..=input).sum()
        }
    }
    impl FixtureExecutor for ClosedForm {
        fn delivered(&self, input: u64) -> u64 {
            input * (input + 1) / 2
        }
    }

    fn id(name: &str) -> ContentId {
        ContentId::from_bytes(
            Symbol::qualified("fixture", name),
            Sha256::digest(name).into(),
        )
    }
    fn evidence(name: &str, quality: f64, cost: u64) -> SubjectEvidence {
        SubjectEvidence {
            subject: id(name),
            values: BTreeMap::from([
                (Symbol::new("quality"), quality),
                (Symbol::new("cost"), cost as f64),
            ]),
            evidence: BTreeSet::from([id(&format!("evidence-{name}"))]),
            inferences: BTreeSet::from([id(&format!("inference-{name}"))]),
            attributions: BTreeSet::from([id(&format!("attribution-{name}"))]),
            epochs: BTreeSet::from([
                id(&format!("epoch-{name}-0")),
                id(&format!("epoch-{name}-1")),
            ]),
            spend: summarize_spend(&[AttributedResourceEvent {
                evidence: id("spend"),
                subject: Some(id(name)),
                route: Some(id("fixture")),
                resource: Symbol::new("steps"),
                amount: cost,
                cause: ResourceCause::CandidateRoute,
            }]),
            gate_rejected: false,
            unresolved: false,
            bootstrap: false,
            private_unapproved: false,
            report_only: false,
            quarantined: false,
        }
    }

    #[test]
    fn non_ai_product_specimen_runs_smoke_screen_confirmation_decision_and_selection() {
        let implementations: [(&str, Box<dyn FixtureExecutor>, u64); 2] = [
            ("iterative", Box::new(Iterative), 10),
            ("closed-form", Box::new(ClosedForm), 1),
        ];
        assert!(
            implementations
                .iter()
                .all(|(_, fixture, _)| fixture.delivered(100) == 5050)
        );
        let cells = implementations
            .iter()
            .enumerate()
            .map(|(sample, (name, _, _))| DesignCell {
                id: id(name),
                subject: id(name),
                task: id("sum-1-to-n"),
                sample: sample as u32,
                route: id(&format!("route-{name}")),
                host: id("fixture-host"),
                upper_exposure: 10,
            })
            .collect::<Vec<_>>();
        let controls = BTreeSet::from([cells[0].id.clone()]);
        let seal = DesignSeal::new(
            id("snapshot"),
            cells,
            vec![1],
            [9; 32],
            controls.clone(),
            EliminationPolicy {
                id: id("elimination"),
                total_error_ppm: 20_000,
            },
            ResolutionPolicy {
                id: id("resolution"),
                max_selected_cells: 1,
            },
            ConfirmationDesign {
                id: id("confirmation"),
                required_controls: controls,
            },
            0,
            1,
            20,
        )
        .unwrap();
        assert!(
            seal.pool
                .keys()
                .all(|cell| smoke(&seal, cell, true, true).unwrap() == SmokeDiagnosis::Ready)
        );
        let mut screen = SequentialScreen::new(&seal);
        screen
            .decide(&id("iterative"), 1, 10_000, ScreeningDecision::Eliminate)
            .unwrap();
        screen
            .decide(&id("closed-form"), 1, 10_000, ScreeningDecision::Keep)
            .unwrap();
        let survivors = screen.survivors();
        let confirmation = seal_confirmation(&seal, &survivors, &BTreeSet::new()).unwrap();
        assert!(
            confirmation.cells.contains(&id("closed-form"))
                && confirmation.cells.contains(&id("iterative"))
        );
        let rows = implementations
            .iter()
            .map(|(name, _, cost)| evidence(name, 1.0, *cost))
            .collect::<Vec<_>>();
        let report = decide(
            DecisionSpec {
                id: id("decision"),
                evidence_root: id("evidence-root"),
                dimensions: vec![
                    DimensionSpec {
                        name: Symbol::new("quality"),
                        direction: Direction::Maximize,
                        equivalence: 0.0,
                        required: true,
                    },
                    DimensionSpec {
                        name: Symbol::new("cost"),
                        direction: Direction::Minimize,
                        equivalence: 0.0,
                        required: true,
                    },
                ],
                budget: Some(20),
                negative_epoch_floor: 2,
                expiry: id("expiry"),
            },
            rows.clone(),
        )
        .unwrap();
        let selection = select(
            &report,
            &rows,
            StalenessInputs {
                subject_snapshot: id("snapshot"),
                evidence_root: id("evidence-root"),
                policy: id("decision"),
            },
        )
        .unwrap();
        assert_eq!(selection.subjects, vec![id("closed-form")]);
    }
}
