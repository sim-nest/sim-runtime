//! Canonical `CHARACTERIZE_1` class-semantic scenarios.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureMode {
    UnknownParent,
    DuplicateParent,
    InconsistentC3,
    ParentCycle,
    ReadConstructorMissing,
    ReadShapeMismatch,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScenarioKind {
    C3Resolution,
    DeclaredParentTraversal,
    SubclassTest,
    ReadConstruction,
    Failure(FailureMode),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExpectedOutcome {
    Linearization(&'static [&'static str]),
    Traversal(&'static [&'static str]),
    Subclass(bool),
    Constructed(&'static str),
    Rejected(&'static str),
}

/// One canonical scenario. `canonical` is the complete stable capture payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CharacterizationScenario {
    pub id: &'static str,
    pub kind: ScenarioKind,
    pub canonical: &'static str,
    pub expected: ExpectedOutcome,
}

pub const fn characterization_scenarios() -> &'static [CharacterizationScenario] {
    &[
        CharacterizationScenario {
            id: "characterize-1/c3-diamond",
            kind: ScenarioKind::C3Resolution,
            canonical: "class O(); class A(O); class B(O); class C(A,B)",
            expected: ExpectedOutcome::Linearization(&["C", "A", "B", "O"]),
        },
        CharacterizationScenario {
            id: "characterize-1/declared-parent-depth-first",
            kind: ScenarioKind::DeclaredParentTraversal,
            canonical: "class Root(); class Left(Root); class Right(Root); class Leaf(Left,Right)",
            expected: ExpectedOutcome::Traversal(&["Left", "Root", "Right", "Root"]),
        },
        CharacterizationScenario {
            id: "characterize-1/subclass-transitive",
            kind: ScenarioKind::SubclassTest,
            canonical: "class Root(); class Middle(Root); class Leaf(Middle); subclass(Leaf,Root)",
            expected: ExpectedOutcome::Subclass(true),
        },
        CharacterizationScenario {
            id: "characterize-1/subclass-unrelated",
            kind: ScenarioKind::SubclassTest,
            canonical: "class Left(); class Right(); subclass(Left,Right)",
            expected: ExpectedOutcome::Subclass(false),
        },
        CharacterizationScenario {
            id: "characterize-1/read-construction",
            kind: ScenarioKind::ReadConstruction,
            canonical: "class Point(x:i64,y:i64); read #(Point 3 5)",
            expected: ExpectedOutcome::Constructed("Point{x=3,y=5}"),
        },
        CharacterizationScenario {
            id: "characterize-1/fail-unknown-parent",
            kind: ScenarioKind::Failure(FailureMode::UnknownParent),
            canonical: "class Child(Missing)",
            expected: ExpectedOutcome::Rejected("class/unknown-parent"),
        },
        CharacterizationScenario {
            id: "characterize-1/fail-duplicate-parent",
            kind: ScenarioKind::Failure(FailureMode::DuplicateParent),
            canonical: "class Child(Base,Base)",
            expected: ExpectedOutcome::Rejected("class/duplicate-parent"),
        },
        CharacterizationScenario {
            id: "characterize-1/fail-inconsistent-c3",
            kind: ScenarioKind::Failure(FailureMode::InconsistentC3),
            canonical: "class A(X,Y); class B(Y,X); class Z(A,B)",
            expected: ExpectedOutcome::Rejected("class/inconsistent-c3"),
        },
        CharacterizationScenario {
            id: "characterize-1/fail-parent-cycle",
            kind: ScenarioKind::Failure(FailureMode::ParentCycle),
            canonical: "class A(B); class B(A)",
            expected: ExpectedOutcome::Rejected("class/parent-cycle"),
        },
        CharacterizationScenario {
            id: "characterize-1/fail-read-constructor-missing",
            kind: ScenarioKind::Failure(FailureMode::ReadConstructorMissing),
            canonical: "class Opaque(); read #(Opaque)",
            expected: ExpectedOutcome::Rejected("class/read-constructor-missing"),
        },
        CharacterizationScenario {
            id: "characterize-1/fail-read-shape",
            kind: ScenarioKind::Failure(FailureMode::ReadShapeMismatch),
            canonical: "class Point(x:i64,y:i64); read #(Point three 5)",
            expected: ExpectedOutcome::Rejected("class/read-shape-mismatch"),
        },
    ]
}

/// Stable 256-bit identity over the schema, id, input, and expected observation.
pub fn scenario_content_id(scenario: &CharacterizationScenario) -> [u8; 32] {
    let observation = format!("{:?}", scenario.expected);
    let fields = [
        b"sim.class-characterization/v1".as_slice(),
        scenario.id.as_bytes(),
        scenario.canonical.as_bytes(),
        observation.as_bytes(),
    ];
    let mut output = [0_u8; 32];
    for lane in 0..4 {
        let mut hash = 0xcbf29ce484222325_u64 ^ (lane as u64).wrapping_mul(0x9e3779b97f4a7c15);
        for field in fields {
            for byte in field {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x100000001b3);
            }
            hash ^= 0xff;
            hash = hash.wrapping_mul(0x100000001b3);
        }
        output[lane * 8..(lane + 1) * 8].copy_from_slice(&hash.to_be_bytes());
    }
    output
}
