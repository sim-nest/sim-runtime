//! Bounded, evidence-carrying handler class selection.

use sim_kernel::{ClassId, ClassRef, Cx, Result};

use crate::Raised;

/// Finite work allowance passed unchanged to the class-relation provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassMatchBudget {
    /// Maximum class-relation work admitted by the caller.
    pub work: usize,
}

/// Inspectable proof retained from a bounded subclass query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassMatchEvidence {
    /// Stable raised-class identity tested by the provider.
    pub raised: ClassId,
    /// Stable candidate handler identity tested by the provider.
    pub candidate: ClassId,
    /// Work charged by the bounded provider.
    pub performed_work: usize,
}

/// Exact result supplied by a bounded class-relation provider such as CLASS_2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoundedSubclassOutcome {
    /// Positive subclass evidence.
    Subclass(ClassMatchEvidence),
    /// Conclusive negative subclass evidence.
    NotSubclass(ClassMatchEvidence),
    /// The provider rejected a malformed or cyclic parent graph.
    InvalidClassGraph {
        /// Provider explanation of the invalid graph.
        reason: String,
    },
    /// The provider could not decide within the supplied allowance.
    BudgetExhausted {
        /// Configured work ceiling.
        limit: usize,
        /// Work completed before exhaustion.
        performed_work: usize,
    },
}

/// Exact outcome of bounded handler class selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassMatchOutcome {
    /// Both subclass evidence and the language predicate accepted the candidate.
    Matched(ClassMatchEvidence),
    /// Subclass evidence rejected the candidate, or language policy narrowed it.
    NotMatched(ClassMatchEvidence),
    /// The declared parent relation is invalid.
    InvalidClassGraph {
        /// Provider explanation of the invalid graph.
        reason: String,
    },
    /// The finite work allowance was exhausted before an answer existed.
    BudgetExhausted {
        /// Configured work ceiling.
        limit: usize,
        /// Work completed before exhaustion.
        performed_work: usize,
    },
    /// The caller-supplied language predicate could not decide a positive candidate.
    PolicyFailure {
        /// Stable policy error text.
        reason: String,
        /// Positive subclass evidence presented to the policy.
        evidence: ClassMatchEvidence,
    },
}

/// Matches a raised class using bounded subclass evidence and explicit policy.
///
/// `bounded_subclass` is the adapter seam for the owning class organ: it must
/// return its checked, bounded evidence rather than a bare boolean. The
/// `language_predicate` is invoked only for positive subclass evidence. It may
/// narrow a match for language-specific rules, but cannot widen a negative.
pub fn match_raised_class(
    cx: &mut Cx,
    raised: &Raised,
    candidate: ClassRef,
    budget: ClassMatchBudget,
    bounded_subclass: impl FnOnce(
        &mut Cx,
        &ClassRef,
        &ClassRef,
        ClassMatchBudget,
    ) -> BoundedSubclassOutcome,
    mut language_predicate: impl FnMut(&mut Cx, &Raised, &ClassRef) -> Result<bool>,
) -> ClassMatchOutcome {
    let Some(raised_class) = raised.class_ref().object().as_class() else {
        return ClassMatchOutcome::InvalidClassGraph {
            reason: "Raised class field is not a class object".into(),
        };
    };
    let Some(candidate_class) = candidate.object().as_class() else {
        return ClassMatchOutcome::InvalidClassGraph {
            reason: "candidate handler is not a class object".into(),
        };
    };
    let expected = (raised_class.id(), candidate_class.id());
    match bounded_subclass(cx, raised.class_ref(), &candidate, budget) {
        BoundedSubclassOutcome::NotSubclass(evidence) => {
            match validate_evidence(evidence, expected, budget) {
                Ok(evidence) => ClassMatchOutcome::NotMatched(evidence),
                Err(outcome) => outcome,
            }
        }
        BoundedSubclassOutcome::InvalidClassGraph { reason } => {
            ClassMatchOutcome::InvalidClassGraph { reason }
        }
        BoundedSubclassOutcome::BudgetExhausted {
            limit,
            performed_work,
        } => ClassMatchOutcome::BudgetExhausted {
            limit,
            performed_work,
        },
        BoundedSubclassOutcome::Subclass(evidence) => {
            let evidence = match validate_evidence(evidence, expected, budget) {
                Ok(evidence) => evidence,
                Err(outcome) => return outcome,
            };
            match language_predicate(cx, raised, &candidate) {
                Ok(true) => ClassMatchOutcome::Matched(evidence),
                Ok(false) => ClassMatchOutcome::NotMatched(evidence),
                Err(error) => ClassMatchOutcome::PolicyFailure {
                    reason: error.to_string(),
                    evidence,
                },
            }
        }
    }
}

fn validate_evidence(
    evidence: ClassMatchEvidence,
    expected: (ClassId, ClassId),
    budget: ClassMatchBudget,
) -> std::result::Result<ClassMatchEvidence, ClassMatchOutcome> {
    if (evidence.raised, evidence.candidate) != expected {
        return Err(ClassMatchOutcome::PolicyFailure {
            reason: "bounded subclass evidence names different class identities".into(),
            evidence,
        });
    }
    if evidence.performed_work > budget.work {
        return Err(ClassMatchOutcome::PolicyFailure {
            reason: "bounded subclass evidence exceeds the supplied work budget".into(),
            evidence,
        });
    }
    Ok(evidence)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_kernel::{
        Args, Callable, Class, CodecId, Error, Object, ObjectCompat, Origin, ReadConstructorRef,
        ShapeRef, SourceId, Span, Symbol, TableRef, Value,
    };

    use super::*;

    struct TestClass {
        id: ClassId,
        display: &'static str,
    }

    impl Object for TestClass {
        fn display(&self, _cx: &mut Cx) -> Result<String> {
            Ok(self.display.into())
        }
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }
    impl ObjectCompat for TestClass {
        fn class(&self, _cx: &mut Cx) -> Result<ClassRef> {
            Err(Error::Eval("unused".into()))
        }
        fn as_callable(&self) -> Option<&dyn Callable> {
            Some(self)
        }
        fn as_class(&self) -> Option<&dyn Class> {
            Some(self)
        }
    }
    impl Callable for TestClass {
        fn call(&self, _cx: &mut Cx, _args: Args) -> Result<Value> {
            Err(Error::Eval("unused".into()))
        }
    }
    impl Class for TestClass {
        fn id(&self) -> ClassId {
            self.id
        }
        fn symbol(&self) -> Symbol {
            Symbol::qualified("test", self.display)
        }
        fn constructor_shape(&self, _cx: &mut Cx) -> Result<ShapeRef> {
            Err(Error::Eval("unused".into()))
        }
        fn instance_shape(&self, _cx: &mut Cx) -> Result<ShapeRef> {
            Err(Error::Eval("unused".into()))
        }
        fn read_constructor(&self, _cx: &mut Cx) -> Result<Option<ReadConstructorRef>> {
            Ok(None)
        }
        fn members(&self, _cx: &mut Cx) -> Result<TableRef> {
            Err(Error::Eval("unused".into()))
        }
    }

    fn class(cx: &mut Cx, id: u32, display: &'static str) -> ClassRef {
        cx.factory()
            .opaque(Arc::new(TestClass {
                id: ClassId(id),
                display,
            }))
            .unwrap()
    }

    fn raised(cx: &mut Cx, class: ClassRef, profile: &str) -> Raised {
        Raised::new(
            class,
            cx.factory().string("payload".into()).unwrap(),
            Origin {
                codec: CodecId(1),
                source: SourceId("matching-test".into()),
                span: Span { start: 0, end: 0 },
                trivia: Vec::new(),
            },
            Symbol::qualified("test", profile),
        )
        .unwrap()
    }

    fn evidence(raised: ClassId, candidate: ClassId, performed_work: usize) -> ClassMatchEvidence {
        ClassMatchEvidence {
            raised,
            candidate,
            performed_work,
        }
    }

    #[test]
    fn three_deep_hierarchy_matches_at_each_level_from_bounded_evidence() {
        let mut cx = sim_kernel::testing::bare_cx();
        let classes = [
            class(&mut cx, 9100, "Root"),
            class(&mut cx, 9101, "Middle"),
            class(&mut cx, 9102, "Leaf"),
        ];
        let raised = raised(&mut cx, classes[2].clone(), "profile");
        for (work, candidate) in classes.into_iter().rev().enumerate() {
            let raised_id = raised.class_ref().object().as_class().unwrap().id();
            let candidate_id = candidate.object().as_class().unwrap().id();
            let outcome = match_raised_class(
                &mut cx,
                &raised,
                candidate,
                ClassMatchBudget { work: 8 },
                |_, _, _, _| {
                    BoundedSubclassOutcome::Subclass(evidence(raised_id, candidate_id, work + 1))
                },
                |_, _, _| Ok(true),
            );
            assert!(matches!(outcome, ClassMatchOutcome::Matched(_)));
        }
    }

    #[test]
    fn invalid_graph_and_exhaustion_remain_distinct_from_negative() {
        let mut cx = sim_kernel::testing::bare_cx();
        let class = class(&mut cx, 9110, "Cycle");
        let raised = raised(&mut cx, class.clone(), "profile");
        let invalid = match_raised_class(
            &mut cx,
            &raised,
            class.clone(),
            ClassMatchBudget { work: 8 },
            |_, _, _, _| BoundedSubclassOutcome::InvalidClassGraph {
                reason: "cycle: A -> B -> A".into(),
            },
            |_, _, _| Ok(true),
        );
        assert!(matches!(
            invalid,
            ClassMatchOutcome::InvalidClassGraph { .. }
        ));

        let exhausted = match_raised_class(
            &mut cx,
            &raised,
            class,
            ClassMatchBudget { work: 1 },
            |_, _, _, budget| BoundedSubclassOutcome::BudgetExhausted {
                limit: budget.work,
                performed_work: 1,
            },
            |_, _, _| Ok(true),
        );
        assert!(matches!(
            exhausted,
            ClassMatchOutcome::BudgetExhausted { .. }
        ));
    }

    #[test]
    fn negative_evidence_cannot_be_widened_by_display_or_profile_policy() {
        let mut cx = sim_kernel::testing::bare_cx();
        let raised_class = class(&mut cx, 9120, "Same");
        let candidate = class(&mut cx, 9121, "Same");
        let raised_id = raised_class.object().as_class().unwrap().id();
        let candidate_id = candidate.object().as_class().unwrap().id();
        let raised = raised(&mut cx, raised_class, "Symbol");
        let mut policy_called = false;
        let outcome = match_raised_class(
            &mut cx,
            &raised,
            candidate,
            ClassMatchBudget { work: 8 },
            |_, _, _, _| BoundedSubclassOutcome::NotSubclass(evidence(raised_id, candidate_id, 1)),
            |_, _, _| {
                policy_called = true;
                Ok(true)
            },
        );
        assert!(matches!(outcome, ClassMatchOutcome::NotMatched(_)));
        assert!(
            !policy_called,
            "policy must not widen negative subclass evidence"
        );
        let production_source = include_str!("matching.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(!production_source.contains(".display("));
        assert!(!production_source.contains(".profile()"));
    }
}
