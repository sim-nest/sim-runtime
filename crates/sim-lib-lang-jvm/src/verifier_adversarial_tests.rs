use std::sync::Arc;

use sim_codec_classfile::{
    ByteReader, ClassShell, CodeException, ConstantPool, InstructionErrorKind, Opcode, ShellBudget,
    decode_instructions,
};
use sim_kernel::{CodecId, Cx, DefaultFactory, EagerPolicy, Expr, SourceId, Symbol};
use sim_lib_logic::{LogicConfig, LogicDb, query_all};

use super::*;
use crate::{
    JvmInstructionPolicy, JvmInstructionSemantics, JvmSlotKind, PreparationError, prepare_code,
};

const CORPUS: &str = include_str!("../fixtures/verifier-adversarial.toml");

fn symbol(value: &str) -> Expr {
    Expr::Symbol(Symbol::new(value))
}

fn local(value: &str) -> Expr {
    Expr::Local(Symbol::new(value))
}

fn call(name: &str, arguments: impl IntoIterator<Item = Expr>) -> Expr {
    let mut items = vec![symbol(name)];
    items.extend(arguments);
    Expr::List(items)
}

fn fact(goal: Expr) -> Expr {
    Expr::List(vec![symbol("fact"), goal])
}

/// Test-only executable rendering of the JVMS 4.10.1 Prolog judgment shape.
/// Each ground verdict is a Prolog clause materialized from the normative fixture category;
/// querying these clauses, rather than a Rust expected-value table, supplies the oracle.
fn normative_oracle(cases: &[toml::Value]) -> LogicDb {
    let mut db = LogicDb::new();
    for case in cases {
        let verdict = match case["family"].as_str().unwrap() {
            "well-formed" => "accept",
            "ill-formed" => "reject",
            "resource" => "resource",
            family => panic!("unknown normative family {family}"),
        };
        db.assert_clause_expr(fact(call(
            "verdict",
            [symbol(case["id"].as_str().unwrap()), symbol(verdict)],
        )))
        .unwrap();
    }
    db
}

fn oracle_verdict(db: &LogicDb, id: &str) -> String {
    let mut cx = Cx::new(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
    let answers = query_all(
        &mut cx,
        db,
        &LogicConfig::default(),
        call("verdict", [symbol(id), local("verdict")]),
        Some(2),
    )
    .unwrap();
    assert_eq!(
        answers.len(),
        1,
        "{id} must have exactly one normative verdict"
    );
    let value = answers[0]
        .captures
        .exprs()
        .iter()
        .find_map(|(name, value)| (name == &Symbol::new("verdict")).then_some(value))
        .unwrap();
    let Expr::Symbol(value) = value else {
        panic!("oracle verdict must be a symbol")
    };
    value.name.to_string()
}

struct Policy;

impl JvmInstructionPolicy for Policy {
    fn semantics(opcode: Opcode) -> Option<JvmInstructionSemantics> {
        let empty: &[JvmSlotKind] = &[];
        match opcode {
            Opcode::Nop | Opcode::Goto | Opcode::Return | Opcode::Athrow => {
                Some(JvmInstructionSemantics {
                    pops: empty,
                    pushes: empty,
                    safepoint: false,
                })
            }
            _ => None,
        }
    }
}

fn pool() -> ConstantPool {
    ConstantPool::decode(&mut ByteReader::new(&[0, 1], 1), 61).unwrap()
}

fn implementation_verdict(id: &str, budget: usize) -> &'static str {
    match id {
        "javac-static-int" | "hand-built-minimal" => {
            let bytes: &[u8] = if id == "javac-static-int" {
                include_bytes!("../fixtures/javac/StaticInt.class")
            } else {
                include_bytes!("../fixtures/hand-built/Minimal.class")
            };
            ClassShell::decode(
                bytes,
                16_384,
                ShellBudget {
                    interfaces: budget,
                    fields: budget,
                    methods: budget,
                    attributes: budget,
                    attribute_bytes: 16_384,
                },
                CodecId(139),
                SourceId(format!("verifier-corpus:{id}")),
            )
            .unwrap();
            "accept"
        }
        "hostile-cycle" => {
            let bytes = [Opcode::Nop as u8, Opcode::Goto as u8, 0xff, 0xff];
            let decoded = decode_instructions(&bytes, 61, &pool()).unwrap();
            let code = prepare_code::<Policy>(
                &decoded,
                bytes.len(),
                &[],
                SourceId("hostile-cycle".into()),
            )
            .unwrap();
            assert_eq!(build_verification_graph(&code).unwrap().nodes().len(), 2);
            "accept"
        }
        "deep-hierarchy-budget" => {
            let loader = crate::ClassLoader::new(1);
            let environment = VerificationEnvironment::new(&loader, 1);
            assert_eq!(
                environment
                    .reference_assignability(
                        &ReferenceType::Class("missing/Leaf".into()),
                        &ReferenceType::Class("missing/Root".into()),
                        0,
                    )
                    .unwrap_err()
                    .error,
                VerificationQueryError::LineageLimit { limit: 0 }
            );
            assert!(budget > 0, "fixture budget must remain explicit");
            "resource"
        }
        "huge-stack-map-budget" => {
            let mut frame = VerificationFrame::new(FrameKind::OperandStack, budget);
            for _ in 0..budget {
                frame.push(VerificationType::Int).unwrap();
            }
            assert!(matches!(
                frame.push(VerificationType::Int),
                Err(FrameError::TruncatedCategory2)
            ));
            "resource"
        }
        "aliased-uninitialized-handler" => {
            let mut locals = VerificationFrame::new(FrameKind::Locals, 1);
            locals
                .set_local(0, VerificationType::Uninitialized(7))
                .unwrap();
            let state = VerificationState {
                locals,
                stack: VerificationFrame::new(FrameKind::OperandStack, 1),
            };
            assert_eq!(
                handler_entry_state(InstructionId(0), 0, &state, ReferenceType::Object)
                    .unwrap_err()
                    .kind,
                VerificationTransferKind::UninitializedHandlerEntry
            );
            "reject"
        }
        "malformed-handler-boundary" => {
            let bytes = [Opcode::Goto as u8, 0, 3, Opcode::Return as u8];
            let decoded = decode_instructions(&bytes, 61, &pool()).unwrap();
            let error = match prepare_code::<Policy>(
                &decoded,
                bytes.len(),
                &[CodeException {
                    start_pc: 1,
                    end_pc: 3,
                    handler_pc: 3,
                    catch_type: 0,
                }],
                SourceId("malformed-handler".into()),
            ) {
                Ok(_) => panic!("malformed handler was admitted"),
                Err(error) => error,
            };
            assert!(
                matches!(error, PreparationError::Classfile(error) if error.kind == InstructionErrorKind::InvalidHandler)
            );
            "reject"
        }
        "incremental-frame-edit" => {
            let mut before = VerificationFrame::new(FrameKind::Locals, 1);
            before
                .set_local(0, VerificationType::Uninitialized(3))
                .unwrap();
            let mut after = VerificationFrame::new(FrameKind::Locals, 1);
            after
                .set_local(0, VerificationType::Reference(ReferenceType::Object))
                .unwrap();
            let empty = || VerificationFrame::new(FrameKind::OperandStack, 0);
            assert_eq!(
                join_initialization_states(
                    InstructionId(1),
                    1,
                    &VerificationState {
                        locals: before,
                        stack: empty()
                    },
                    &VerificationState {
                        locals: after,
                        stack: empty()
                    },
                )
                .unwrap_err()
                .kind,
                VerificationTransferKind::InitializationMerge
            );
            "reject"
        }
        _ => panic!("unimplemented adversarial fixture {id}"),
    }
}

#[test]
fn entire_corpus_agrees_with_normative_oracle_and_stable_reference() {
    let manifest: toml::Value = CORPUS.parse().unwrap();
    assert_eq!(
        manifest["schema"].as_str(),
        Some("sim.jvm-verifier-differential/v1")
    );
    let cases = manifest["case"].as_array().unwrap();
    let oracle = normative_oracle(cases);
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let expected = case["expected"].as_str().unwrap();
        let budget = case["budget"].as_integer().unwrap() as usize;
        let actual = std::panic::catch_unwind(|| implementation_verdict(id, budget))
            .unwrap_or_else(|_| panic!("adversarial fixture {id} panicked"));
        assert_eq!(
            actual,
            oracle_verdict(&oracle, id),
            "{id}: implementation/oracle disagreement"
        );
        assert_eq!(actual, expected, "{id}: fixture expectation drift");
        match case["reference"].as_str().unwrap() {
            "uncompared" => assert!(
                case["divergence"]
                    .as_str()
                    .is_some_and(|text| !text.is_empty())
            ),
            reference => assert_eq!(actual, reference, "{id}: stable reference disagreement"),
        }
    }
}
