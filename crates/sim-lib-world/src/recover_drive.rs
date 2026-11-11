// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Published recovery calls for one recover case.

use std::collections::BTreeMap;

use sim_kernel::{ContentId, Symbol};
use sim_lib_roadmap_runner::{
    MutationError, PathState, PortableImage, RecoveryEffect, RecoveryStop, ResumeDecision,
    SealedMutationPlan, StructuralEdit, classify_image, classify_plan, inverse_plan,
    plan_refinement_after_reconciliation, plan_retry_after_reconciliation,
    terminal_requests_no_effects,
};
use sim_roadmap_exec_core::{
    AttemptId, ClassifiedFailure, FailureClass, RecoveryPolicy, RetryContext, RetryDecision,
    RetryRule, StopReason,
};

pub(crate) struct Law {
    pub(crate) detail: String,
    pub(crate) invokes: u64,
}

pub(crate) enum Fault {
    Bound,
    Refused,
}

impl Fault {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::Bound => "bound",
            Self::Refused => "recover-refused",
        }
    }
}

pub(crate) fn stop() -> Result<Law, Fault> {
    let mut invokes = 0;
    let policy = RecoveryPolicy::default();
    let failure = ClassifiedFailure::unknown(Vec::new());
    let ambiguous = ResumeDecision::Ambiguous {
        foreign_paths: vec!["foreign.rs".to_owned()],
    };
    let retry = call(
        &mut invokes,
        plan_retry_after_reconciliation(&ambiguous, &policy, &failure, &context(0, false, true)?),
    );
    let ambiguous_stops = matches!(
        retry,
        RecoveryEffect::Stop(RecoveryStop::Ambiguous { ref foreign_paths })
            if foreign_paths.as_slice() == ["foreign.rs"]
    ) && call(&mut invokes, terminal_requests_no_effects(&retry));
    let apply = ResumeDecision::Apply {
        paths: vec!["src/a.txt".to_owned()],
    };
    let unreconciled = call(
        &mut invokes,
        plan_retry_after_reconciliation(&apply, &policy, &failure, &context(0, false, true)?),
    );
    let apply_stops = matches!(
        unreconciled,
        RecoveryEffect::Stop(RecoveryStop::MutationNotReconciled)
    ) && call(&mut invokes, terminal_requests_no_effects(&unreconciled));
    let unresolved = call(
        &mut invokes,
        plan_retry_after_reconciliation(
            &ResumeDecision::Committed,
            &policy,
            &failure,
            &context(0, true, true)?,
        ),
    );
    let unresolved_stops = matches!(
        unresolved,
        RecoveryEffect::Stop(RecoveryStop::Retry(StopReason::UnresolvedEffect))
    ) && call(&mut invokes, terminal_requests_no_effects(&unresolved));
    let changed = call(
        &mut invokes,
        plan_retry_after_reconciliation(
            &ResumeDecision::Committed,
            &policy,
            &failure,
            &context(0, false, false)?,
        ),
    );
    let changed_stops = matches!(
        changed,
        RecoveryEffect::Stop(RecoveryStop::Retry(StopReason::IdentitiesChanged))
    ) && call(&mut invokes, terminal_requests_no_effects(&changed));
    if ambiguous_stops && apply_stops && unresolved_stops && changed_stops {
        return Ok(Law {
            detail: "ambiguous unreconciled unresolved changed same 1".to_owned(),
            invokes,
        });
    }
    Err(Fault::Refused)
}

pub(crate) fn admit() -> Result<Law, Fault> {
    let mut invokes = 0;
    let empty = RecoveryPolicy::default();
    let policy = retry_policy();
    let committed = ResumeDecision::Committed;
    let blocked = call(
        &mut invokes,
        plan_retry_after_reconciliation(
            &committed,
            &empty,
            &failure(FailureClass::Mutation),
            &context(0, false, true)?,
        ),
    );
    let not_retryable = matches!(
        blocked,
        RecoveryEffect::Stop(RecoveryStop::Retry(StopReason::ClassNotRetryable))
    ) && call(&mut invokes, terminal_requests_no_effects(&blocked));
    let unnamed_effect = call(
        &mut invokes,
        plan_retry_after_reconciliation(
            &committed,
            &empty,
            &failure(FailureClass::Proof),
            &context(0, false, true)?,
        ),
    );
    let unnamed = matches!(
        unnamed_effect,
        RecoveryEffect::Stop(RecoveryStop::Retry(StopReason::ClassNotNamed))
    ) && call(&mut invokes, terminal_requests_no_effects(&unnamed_effect));
    let exhausted_effect = call(
        &mut invokes,
        plan_retry_after_reconciliation(
            &committed,
            &policy,
            &failure(FailureClass::InfrastructureTransient),
            &context(1, false, true)?,
        ),
    );
    let exhausted = matches!(
        exhausted_effect,
        RecoveryEffect::Stop(RecoveryStop::Retry(StopReason::AttemptsExhausted))
    ) && call(
        &mut invokes,
        terminal_requests_no_effects(&exhausted_effect),
    );
    let admitted = call(
        &mut invokes,
        plan_retry_after_reconciliation(
            &committed,
            &policy,
            &failure(FailureClass::InfrastructureTransient),
            &context(0, false, true)?,
        ),
    );
    let retry = matches!(
        admitted,
        RecoveryEffect::Retry(RetryDecision::Retry(ref receipt))
            if receipt.next_attempt_number == 1
                && receipt.remaining_attempts == 0
                && receipt.backoff_millis == 5
    ) && !call(&mut invokes, terminal_requests_no_effects(&admitted));
    if not_retryable && unnamed && exhausted && retry {
        return Ok(Law {
            detail: "not-retryable unnamed exhausted retry same 1".to_owned(),
            invokes,
        });
    }
    Err(Fault::Refused)
}

pub(crate) fn descent() -> Result<Law, Fault> {
    let mut invokes = 0;
    let policy = RecoveryPolicy {
        max_refinement_rank: 4,
        ..RecoveryPolicy::default()
    };
    let profile = cid(7);
    let invoked = call(
        &mut invokes,
        plan_refinement_after_reconciliation(
            &ResumeDecision::Committed,
            true,
            profile.clone(),
            4,
            3,
            &policy,
        ),
    );
    let refiner = matches!(
        invoked,
        RecoveryEffect::InvokeRefiner { ref derived_profile } if derived_profile == &profile
    ) && !call(&mut invokes, terminal_requests_no_effects(&invoked));
    let mut terminal = true;
    for (fresh, parent, child) in [(false, 4, 3), (true, 4, 4), (true, 4, 5)] {
        let effect = call(
            &mut invokes,
            plan_refinement_after_reconciliation(
                &ResumeDecision::Committed,
                fresh,
                cid(7),
                parent,
                child,
                &policy,
            ),
        );
        terminal &= matches!(
            effect,
            RecoveryEffect::Stop(RecoveryStop::RefinementNotLowerRank)
        ) && call(&mut invokes, terminal_requests_no_effects(&effect));
    }
    if refiner && terminal {
        return Ok(Law {
            detail: "refiner terminal same 1".to_owned(),
            invokes,
        });
    }
    Err(Fault::Refused)
}

pub(crate) fn image() -> Result<Law, Fault> {
    let mut invokes = 0;
    let same = PortableImage::file(b"same plan".to_vec(), 0o644);
    let foreign = PortableImage::file(b"independent user edit".to_vec(), 0o644);
    let intended = PortableImage::file(b"intended".to_vec(), 0o644);
    let unchanged = call(&mut invokes, classify_image(&same, &same, &foreign));
    let preimage = call(&mut invokes, classify_image(&same, &intended, &same));
    let postimage = call(&mut invokes, classify_image(&same, &intended, &intended));
    let foreign_state = call(&mut invokes, classify_image(&same, &intended, &foreign));
    if unchanged == PathState::Unchanged
        && preimage == PathState::Preimage
        && postimage == PathState::Postimage
        && foreign_state == PathState::Foreign
    {
        return Ok(Law {
            detail: "unchanged preimage postimage foreign same 1".to_owned(),
            invokes,
        });
    }
    Err(Fault::Refused)
}

pub(crate) fn plan() -> Result<Law, Fault> {
    let mut invokes = 0;
    let pre = PortableImage::file(b"old\n".to_vec(), 0o644);
    let post = PortableImage::file(b"new\n".to_vec(), 0o644);
    let foreign = PortableImage::file(b"other\n".to_vec(), 0o644);
    let sealed = SealedMutationPlan::seal(vec![StructuralEdit {
        path: "src/a.txt".to_owned(),
        preimage: pre.clone(),
        postimage: post.clone(),
    }])
    .map_err(|_| Fault::Bound)?;
    let mut observed = BTreeMap::from([("src/a.txt".to_owned(), post.clone())]);
    let committed = matches!(
        call(&mut invokes, classify_plan(&sealed, &observed)),
        ResumeDecision::Committed
    );
    let inverse =
        call(&mut invokes, inverse_plan(&sealed, &observed)).map_err(|_| Fault::Refused)?;
    let entry = inverse.entries.first().ok_or(Fault::Bound)?;
    let inverted = entry.preimage == post && entry.postimage == pre;
    observed.insert("src/a.txt".to_owned(), pre);
    let apply = matches!(
        call(&mut invokes, classify_plan(&sealed, &observed)),
        ResumeDecision::Apply { ref paths } if paths.as_slice() == ["src/a.txt"]
    );
    observed.insert("src/a.txt".to_owned(), foreign);
    let ambiguous = matches!(
        call(&mut invokes, classify_plan(&sealed, &observed)),
        ResumeDecision::Ambiguous { ref foreign_paths } if foreign_paths.as_slice() == ["src/a.txt"]
    );
    let refused = matches!(
        call(&mut invokes, inverse_plan(&sealed, &observed)),
        Err(MutationError::Ambiguous { ref foreign_paths }) if foreign_paths.as_slice() == ["src/a.txt"]
    );
    if committed && inverted && apply && ambiguous && refused {
        return Ok(Law {
            detail: "committed apply ambiguous same 1".to_owned(),
            invokes,
        });
    }
    Err(Fault::Refused)
}

fn call<T>(invokes: &mut u64, value: T) -> T {
    *invokes += 1;
    value
}

fn context(
    attempts_used: u32,
    unresolved_effect: bool,
    same_identity: bool,
) -> Result<RetryContext, Fault> {
    let now = if same_identity { cid(1) } else { cid(2) };
    Ok(RetryContext {
        attempt: AttemptId::new("attempt-a").map_err(|_| Fault::Bound)?,
        attempts_used,
        unresolved_effect,
        identities_before: vec![cid(1)],
        identities_now: vec![now],
    })
}

fn failure(class: FailureClass) -> ClassifiedFailure {
    ClassifiedFailure {
        class,
        evidence: Vec::new(),
    }
}

fn retry_policy() -> RecoveryPolicy {
    let mut policy = RecoveryPolicy::default();
    policy.retry.insert(
        FailureClass::InfrastructureTransient,
        RetryRule {
            max_attempts: 1,
            backoff_millis: vec![5],
        },
    );
    policy
}

fn cid(n: u8) -> ContentId {
    ContentId::from_bytes(Symbol::qualified("core", "sha256-datum-v1"), [n; 32])
}
