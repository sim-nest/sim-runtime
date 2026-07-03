use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use sim_kernel::{
    Cx, Error, Ref, Result,
    control::{
        ControlAbort, ControlCapture, ControlPolicy, ControlPolicyRef, ControlPrompt,
        ControlResume, aborted_control_result, captured_control_result, resumed_control_result,
    },
};

/// A control policy that allows each captured continuation to resume once.
///
/// Implements the kernel [`ControlPolicy`] contract: prompt entry and abort are
/// unrestricted, but a continuation that has already been resumed cannot be
/// resumed again. This is the default policy installed by the control lib.
pub struct OneShotControlPolicy {
    resumed: Mutex<BTreeSet<Ref>>,
}

impl OneShotControlPolicy {
    /// Builds a one-shot policy with no continuations yet resumed.
    pub fn new() -> Self {
        Self {
            resumed: Mutex::new(BTreeSet::new()),
        }
    }
}

impl Default for OneShotControlPolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl ControlPolicy for OneShotControlPolicy {
    fn name(&self) -> &'static str {
        "one-shot-control"
    }

    fn enter_prompt(&self, _cx: &mut Cx, _prompt: &ControlPrompt) -> Result<()> {
        Ok(())
    }

    fn capture(&self, cx: &mut Cx, capture: &ControlCapture) -> Result<Ref> {
        captured_control_result(cx, capture.continuation.clone(), capture.value.clone())
    }

    fn abort(&self, cx: &mut Cx, abort: &ControlAbort) -> Result<Ref> {
        aborted_control_result(cx, abort.prompt.clone(), abort.value.clone())
    }

    fn resume(&self, cx: &mut Cx, resume: &ControlResume) -> Result<Ref> {
        let mut resumed = self
            .resumed
            .lock()
            .map_err(|_| Error::PoisonedLock("one-shot control policy"))?;
        if !resumed.insert(resume.continuation.clone()) {
            return Err(Error::Eval(
                "one-shot control continuation already resumed".to_owned(),
            ));
        }
        resumed_control_result(cx, resume.continuation.clone(), resume.value.clone())
    }
}

/// A one-shot control policy scoped to a named delimited segment.
///
/// Wraps an [`OneShotControlPolicy`] and tags it with a segment [`Ref`],
/// associating its capture/resume bookkeeping with one delimited region.
pub struct SegmentedControlPolicy {
    segment: Ref,
    one_shot: OneShotControlPolicy,
}

impl SegmentedControlPolicy {
    /// Builds a segmented policy bound to `segment`.
    pub fn new(segment: Ref) -> Self {
        Self {
            segment,
            one_shot: OneShotControlPolicy::new(),
        }
    }

    /// Returns the segment this policy is scoped to.
    pub fn segment(&self) -> &Ref {
        &self.segment
    }
}

impl ControlPolicy for SegmentedControlPolicy {
    fn name(&self) -> &'static str {
        "segmented-control"
    }

    fn enter_prompt(&self, cx: &mut Cx, prompt: &ControlPrompt) -> Result<()> {
        self.one_shot.enter_prompt(cx, prompt)
    }

    fn capture(&self, cx: &mut Cx, capture: &ControlCapture) -> Result<Ref> {
        self.one_shot.capture(cx, capture)
    }

    fn abort(&self, cx: &mut Cx, abort: &ControlAbort) -> Result<Ref> {
        self.one_shot.abort(cx, abort)
    }

    fn resume(&self, cx: &mut Cx, resume: &ControlResume) -> Result<Ref> {
        self.one_shot.resume(cx, resume)
    }
}

/// Builds a shared [`OneShotControlPolicy`] as a kernel
/// [`ControlPolicyRef`](sim_kernel::control::ControlPolicyRef).
pub fn one_shot_control_policy() -> ControlPolicyRef {
    Arc::new(OneShotControlPolicy::new())
}

/// Builds a shared [`SegmentedControlPolicy`] bound to `segment` as a kernel
/// [`ControlPolicyRef`](sim_kernel::control::ControlPolicyRef).
pub fn segmented_control_policy(segment: Ref) -> ControlPolicyRef {
    Arc::new(SegmentedControlPolicy::new(segment))
}

/// Installs the default [`one_shot_control_policy`] into `cx` as the active
/// control policy.
pub fn install_control_policy(cx: &mut Cx) {
    cx.set_control_policy(one_shot_control_policy());
}
