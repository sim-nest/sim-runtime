use std::sync::{Arc, Mutex};

use super::{
    AdmissionLimit, CheckpointError, CleanupStack, FrameError, FrameLimits, JobQueues,
    ResumableFrame, ResumePacket, ResumeResult, Unwind, WorkLimit,
};

#[test]
fn resumable_packet_supports_all_inputs_and_terminal_outcomes() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let driver_seen = seen.clone();
    let mut frame = ResumableFrame::new(
        FrameLimits { depth: 2, work: 2 },
        move |packet: ResumePacket<&'static str, &'static str>, budget: &mut super::StepBudget| {
            budget.charge_work()?;
            driver_seen.lock().unwrap().push(packet.clone());
            match packet {
                ResumePacket::Start => Ok(ResumeResult::Yielded("ready")),
                ResumePacket::Send(value) => Ok(ResumeResult::Yielded(value)),
                ResumePacket::Throw(error) => Ok(ResumeResult::Failed(error)),
                ResumePacket::Close => Ok(ResumeResult::Returned("closed")),
            }
        },
    );
    assert_eq!(
        frame.resume::<_, &str, _>(ResumePacket::Start),
        Ok(ResumeResult::Yielded("ready"))
    );
    assert_eq!(
        frame.resume::<_, &str, _>(ResumePacket::Send("sent")),
        Ok(ResumeResult::Yielded("sent"))
    );
    assert_eq!(
        frame.resume::<&str, _, &str>(ResumePacket::Throw("raised")),
        Ok(ResumeResult::Failed("raised"))
    );
    assert_eq!(
        frame.resume::<_, &str, _>(ResumePacket::Close),
        Err(FrameError::AlreadyComplete)
    );
    assert_eq!(
        *seen.lock().unwrap(),
        vec![
            ResumePacket::Start,
            ResumePacket::Send("sent"),
            ResumePacket::Throw("raised")
        ]
    );

    let mut closing = ResumableFrame::new(
        FrameLimits { depth: 1, work: 1 },
        |packet: ResumePacket<&'static str, &'static str>, budget: &mut super::StepBudget| {
            budget.charge_work()?;
            match packet {
                ResumePacket::Start => Ok(ResumeResult::Yielded("open")),
                ResumePacket::Close => Ok(ResumeResult::Returned("closed")),
                ResumePacket::Send(value) => Ok(ResumeResult::Yielded(value)),
                ResumePacket::Throw(error) => Ok(ResumeResult::Failed(error)),
            }
        },
    );
    assert_eq!(
        closing.resume::<_, &str, &str>(ResumePacket::Start),
        Ok(ResumeResult::Yielded("open"))
    );
    assert_eq!(
        closing.resume::<&str, _, &str>(ResumePacket::Close),
        Ok(ResumeResult::Returned("closed"))
    );
}

#[test]
fn frame_limits_and_double_start_fail_closed() {
    let mut frame = ResumableFrame::new(
        FrameLimits { depth: 0, work: 1 },
        |_packet: ResumePacket<(), ()>, budget: &mut super::StepBudget| {
            budget.enter()?;
            Ok(ResumeResult::<(), (), ()>::Returned(()))
        },
    );
    assert_eq!(
        frame.resume(ResumePacket::Start),
        Err(FrameError::DepthExhausted)
    );
    assert_eq!(
        frame.resume(ResumePacket::Start),
        Err(FrameError::AlreadyStarted)
    );

    let mut frame = ResumableFrame::new(
        FrameLimits { depth: 1, work: 0 },
        |_packet: ResumePacket<(), ()>, budget: &mut super::StepBudget| {
            budget.charge_work()?;
            Ok(ResumeResult::<(), (), ()>::Returned(()))
        },
    );
    assert_eq!(
        frame.resume(ResumePacket::Start),
        Err(FrameError::WorkExhausted)
    );
}

#[test]
fn nested_cleanup_is_lifo_for_every_unwind_reason() {
    type Reason = Unwind<&'static str, &'static str, &'static str, &'static str>;
    let reasons = [
        Reason::Return("return"),
        Reason::Break("break"),
        Reason::Continue("continue"),
        Reason::Exception("exception"),
        Reason::Cancelled,
        Reason::Closed,
    ];
    for reason in reasons {
        let order = Arc::new(Mutex::new(Vec::new()));
        let mut stack = CleanupStack::new();
        for marker in ["outer", "inner"] {
            let order = order.clone();
            stack.push(move |_reason: &Reason| order.lock().unwrap().push(marker));
        }
        assert_eq!(stack.unwind(reason.clone()), reason);
        assert_eq!(*order.lock().unwrap(), vec!["inner", "outer"]);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    Microtask,
    Finalization,
}

#[test]
fn typed_jobs_preserve_fifo_cancel_and_class_isolation() {
    let output = Arc::new(Mutex::new(Vec::new()));
    let mut jobs = JobQueues::new(AdmissionLimit(4));
    let first = {
        let output = output.clone();
        jobs.enqueue(Class::Microtask, move |_| {
            output.lock().unwrap().push("micro-1")
        })
        .unwrap()
    };
    let finalizer = {
        let output = output.clone();
        jobs.enqueue(Class::Finalization, move |_| {
            output.lock().unwrap().push("finalizer")
        })
        .unwrap()
    };
    let cancelled = jobs
        .enqueue(Class::Microtask, |_| panic!("cancelled job ran"))
        .unwrap();
    jobs.cancel(cancelled.id);
    let drain = jobs.drain(Class::Microtask, WorkLimit(2));
    assert_eq!(drain.completed, vec![first.id]);
    assert!(!drain.pending);
    assert_eq!(*output.lock().unwrap(), vec!["micro-1"]);
    assert_eq!(
        jobs.checkpoint(Class::Finalization, WorkLimit(1))
            .unwrap()
            .completed,
        vec![finalizer.id]
    );
    assert_eq!(*output.lock().unwrap(), vec!["micro-1", "finalizer"]);
}

#[test]
fn checkpoint_drains_reentrant_jobs_and_enforces_both_bounds() {
    let output = Arc::new(Mutex::new(Vec::new()));
    let mut jobs = JobQueues::new(AdmissionLimit(3));
    let outer_output = output.clone();
    jobs.enqueue(Class::Microtask, move |jobs| {
        outer_output.lock().unwrap().push("outer");
        let inner_output = outer_output.clone();
        jobs.enqueue(Class::Microtask, move |_| {
            inner_output.lock().unwrap().push("inner")
        })
        .unwrap();
    })
    .unwrap();
    let receipt = jobs.checkpoint(Class::Microtask, WorkLimit(2)).unwrap();
    assert_eq!(receipt.completed.len(), 2);
    assert_eq!(*output.lock().unwrap(), vec!["outer", "inner"]);

    jobs.enqueue(Class::Microtask, |_| {}).unwrap();
    assert_eq!(
        jobs.enqueue(Class::Microtask, |_| {}),
        Err(CheckpointError::AdmissionExhausted)
    );

    let mut jobs = JobQueues::new(AdmissionLimit(2));
    jobs.enqueue(Class::Microtask, |jobs| {
        jobs.enqueue(Class::Microtask, |_| {}).unwrap();
    })
    .unwrap();
    assert_eq!(
        jobs.checkpoint(Class::Microtask, WorkLimit(1)),
        Err(CheckpointError::WorkExhausted)
    );
}
