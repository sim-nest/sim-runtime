// conformance: definite assignment runs through the neutral incremental surface.

//! Definite assignment over the neutral, located register-machine surface.

use std::collections::BTreeSet;

use sim_incremental_core::{
    QueryBudgets, ValueFingerprint,
    dataflow::{
        AdmittedTransfer, Boundary, DataflowEvent, EdgeClass, EdgeSpec, FixpointEngine,
        GraphDirection, JoinSemilattice, LocatedGraphAdapter, NodeSpec, StateSize, TransferPolicy,
    },
};
use sim_kernel::{CodecId, Origin, SourceId, Span};
use sim_lib_machine::{
    BranchTarget, InstructionPolicy, LocatedCode, LocatedInstruction, RegionSpec, SourceLocation,
    TargetLocation,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Instruction {
    id: u8,
    operation: Operation,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Branch,
    Assign(u8),
    Merge,
    Use(u8),
    Loop,
    Return,
    Handler,
    Unreachable,
}

struct Registers;

impl InstructionPolicy for Registers {
    type Instruction = Instruction;
    type InstructionId = u8;

    fn instruction_id(instruction: &Instruction) -> Self::InstructionId {
        instruction.id
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct Location {
    instruction: u8,
    line: u8,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum FlowEdge {
    BranchAssigned,
    BranchBypass,
    AssignedMerge,
    BypassMerge,
    MergeUse,
    UseLoop,
    LoopBack,
    LoopExit,
    ProtectedHandler,
    HandlerReturn,
    UnreachableReturn,
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum FlowClass {
    Exceptional,
}

struct RegisterCfg<'a> {
    code: &'a LocatedCode<Registers>,
}

impl LocatedGraphAdapter for RegisterCfg<'_> {
    type NodeId = u8;
    type EdgeId = FlowEdge;
    type Location = Location;
    type Class = FlowClass;

    fn nodes(&self) -> Vec<NodeSpec<u8, Location>> {
        (1..=9)
            .map(|id| {
                let cursor = self.code.cursor(id).expect("specimen instruction exists");
                assert_eq!(self.code.instruction(cursor).id(), &id);
                assert!(instruction_matches_id(
                    id,
                    self.code.instruction(cursor).instruction().operation
                ));
                NodeSpec {
                    id,
                    location: location(id),
                    boundary: if id == 1 {
                        Boundary::Input
                    } else if id == 7 {
                        Boundary::Output
                    } else {
                        Boundary::Internal
                    },
                }
            })
            .collect()
    }

    fn edges(&self) -> Vec<EdgeSpec<FlowEdge, u8, FlowClass>> {
        let edge = |id, source, target, class| EdgeSpec {
            id,
            source,
            target,
            class,
            direction: GraphDirection::Forward,
        };
        vec![
            edge(FlowEdge::BranchAssigned, 1, 2, EdgeClass::Control),
            edge(FlowEdge::BranchBypass, 1, 3, EdgeClass::Control),
            edge(FlowEdge::AssignedMerge, 2, 4, EdgeClass::Control),
            edge(FlowEdge::BypassMerge, 3, 4, EdgeClass::Control),
            edge(FlowEdge::MergeUse, 4, 5, EdgeClass::Control),
            edge(FlowEdge::UseLoop, 5, 6, EdgeClass::Control),
            edge(FlowEdge::LoopBack, 6, 4, EdgeClass::Control),
            edge(FlowEdge::LoopExit, 6, 7, EdgeClass::Control),
            edge(
                FlowEdge::ProtectedHandler,
                6,
                8,
                EdgeClass::Custom(FlowClass::Exceptional),
            ),
            edge(FlowEdge::HandlerReturn, 8, 7, EdgeClass::Control),
            edge(FlowEdge::UnreachableReturn, 9, 7, EdgeClass::Control),
        ]
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum Fact {
    Assigned { register: u8, at: Location },
    UnassignedBranch { register: u8, at: Location },
    InstructionRevision { instruction: u8, revision: u8 },
}

#[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
struct Facts(BTreeSet<Fact>);

impl Facts {
    fn one(fact: Fact) -> Self {
        Self(BTreeSet::from([fact]))
    }
}

impl StateSize for Facts {
    fn state_size(&self) -> usize {
        self.0.len()
    }
}

impl JoinSemilattice for Facts {
    fn bottom(&self) -> Self {
        Self::default()
    }

    fn join(&self, other: &Self) -> Self {
        Self(self.0.union(&other.0).copied().collect())
    }

    fn less_equal(&self, other: &Self) -> bool {
        self.0.is_subset(&other.0)
    }
}

struct Identity;

impl TransferPolicy<Facts> for Identity {
    fn fingerprint(&self) -> ValueFingerprint {
        ValueFingerprint::new(0x4445_4641_5353_4947)
    }

    fn policy_size(&self) -> usize {
        0
    }

    fn transfer(&self, state: &Facts) -> Facts {
        state.clone()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Diagnostic {
    register: u8,
    merge: Location,
    assigned_branch: Location,
    unassigned_branch: Location,
}

#[test]
fn real_register_machine_proves_definite_assignment_and_edited_cone() {
    let code = register_code();
    let graph = RegisterCfg { code: &code }.build_graph().unwrap();

    assert_eq!(code.branch_targets(1).len(), 2);
    assert_eq!(code.branch_targets(6).len(), 2);
    assert_eq!(code.protected_regions().len(), 1);
    assert!(graph.predecessors(&9).unwrap().is_empty());
    assert!(matches!(
        graph.edge(&FlowEdge::ProtectedHandler).unwrap().class(),
        EdgeClass::Custom(FlowClass::Exceptional)
    ));

    let transfer = AdmittedTransfer::admit(
        Identity,
        &[
            Facts::default(),
            Facts::one(Fact::Assigned {
                register: 0,
                at: location(2),
            }),
            Facts::one(Fact::UnassignedBranch {
                register: 0,
                at: location(3),
            }),
        ],
    )
    .unwrap();
    let seeds = specimen_seeds(1);
    let original = FixpointEngine::solve_proven(
        &graph,
        &transfer,
        Facts::default(),
        seeds.clone(),
        QueryBudgets::unlimited(),
    )
    .unwrap();

    let merge_facts = &original.solution().state(&4).unwrap().0;
    let diagnostic = Diagnostic {
        register: 0,
        merge: location(4),
        assigned_branch: match merge_facts.iter().find_map(|fact| match fact {
            Fact::Assigned { register: 0, at } => Some(*at),
            _ => None,
        }) {
            Some(location) => location,
            None => panic!("assigned branch evidence must reach the merge"),
        },
        unassigned_branch: match merge_facts.iter().find_map(|fact| match fact {
            Fact::UnassignedBranch { register: 0, at } => Some(*at),
            _ => None,
        }) {
            Some(location) => location,
            None => panic!("bypass branch evidence must reach the merge"),
        },
    };
    assert_eq!(
        diagnostic,
        Diagnostic {
            register: 0,
            merge: location(4),
            assigned_branch: location(2),
            unassigned_branch: location(3),
        }
    );
    assert!(original.solution().state(&9).unwrap().0.is_empty());
    assert!(
        original
            .solution()
            .state(&8)
            .unwrap()
            .0
            .contains(&Fact::InstructionRevision {
                instruction: 8,
                revision: 1,
            })
    );

    let incremental = FixpointEngine::solve_incremental(
        &original,
        &graph,
        &transfer,
        Facts::default(),
        specimen_seeds(2),
        QueryBudgets::unlimited(),
    )
    .unwrap();
    let affected = BTreeSet::from([7, 8]);
    let retained_event_count = original
        .solution()
        .events()
        .iter()
        .filter(|event| match event {
            DataflowEvent::Visit(node) => !affected.contains(node),
            DataflowEvent::Propagate { edge, .. } => {
                !affected.contains(graph.edge(edge).unwrap().source())
            }
        })
        .count();
    let revisited = incremental
        .solution()
        .events()
        .iter()
        .skip(retained_event_count)
        .filter_map(|event| match event {
            DataflowEvent::Visit(node) => Some(*node),
            _ => None,
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(revisited, affected);
    assert!(
        incremental.solution().events()[retained_event_count..]
            .iter()
            .all(|event| match event {
                DataflowEvent::Visit(node) => matches!(node, 7 | 8),
                DataflowEvent::Propagate { edge, .. } => matches!(edge, FlowEdge::HandlerReturn),
            })
    );
}

fn specimen_seeds(revision: u8) -> Vec<(u8, Facts)> {
    vec![
        (
            2,
            Facts::one(Fact::Assigned {
                register: 0,
                at: location(2),
            }),
        ),
        (
            3,
            Facts::one(Fact::UnassignedBranch {
                register: 0,
                at: location(3),
            }),
        ),
        (
            8,
            Facts::one(Fact::InstructionRevision {
                instruction: 8,
                revision,
            }),
        ),
    ]
}

fn register_code() -> LocatedCode<Registers> {
    let operations = [
        Operation::Branch,
        Operation::Assign(0),
        Operation::Branch,
        Operation::Merge,
        Operation::Use(0),
        Operation::Loop,
        Operation::Return,
        Operation::Handler,
        Operation::Unreachable,
    ];
    LocatedCode::freeze(
        operations
            .into_iter()
            .enumerate()
            .map(|(index, operation)| {
                let id = u8::try_from(index + 1).unwrap();
                LocatedInstruction::new(
                    Instruction { id, operation },
                    id,
                    SourceLocation::Bytes(Origin {
                        codec: CodecId(1),
                        source: SourceId("definite-assignment-register-machine".into()),
                        span: Span {
                            start: index,
                            end: index + 1,
                        },
                        trivia: vec![],
                    }),
                    false,
                    None,
                )
            })
            .collect(),
        vec![
            BranchTarget {
                from: 1,
                to: TargetLocation::Instruction(2),
            },
            BranchTarget {
                from: 1,
                to: TargetLocation::Instruction(3),
            },
            BranchTarget {
                from: 6,
                to: TargetLocation::Instruction(4),
            },
            BranchTarget {
                from: 6,
                to: TargetLocation::Instruction(7),
            },
        ],
        vec![RegionSpec {
            start: 6,
            end: Some(7),
            handler: TargetLocation::Instruction(8),
        }],
    )
    .unwrap()
}

const fn instruction_matches_id(id: u8, operation: Operation) -> bool {
    matches!(
        (id, operation),
        (1 | 3, Operation::Branch)
            | (2, Operation::Assign(0))
            | (4, Operation::Merge)
            | (5, Operation::Use(0))
            | (6, Operation::Loop)
            | (7, Operation::Return)
            | (8, Operation::Handler)
            | (9, Operation::Unreachable)
    )
}

const fn location(instruction: u8) -> Location {
    Location {
        instruction,
        line: instruction,
    }
}
