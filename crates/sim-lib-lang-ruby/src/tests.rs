use std::sync::Arc;

use sim_kernel::{
    Cx, DefaultFactory, NoopEvalPolicy, Ref, Symbol,
    capability::control_capture_capability,
    control::{control_aborted_status, control_result_status},
};
use sim_lib_standard_core::ProfileRegistry;

use crate::*;

fn cx() -> Cx {
    let mut cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
    sim_lib_control::install_control_policy(&mut cx);
    cx
}

#[test]
fn ruby_break_and_next_reuse_control_organ() {
    let mut cx = cx();
    cx.grant(control_capture_capability());
    let scope = RubyBlockScope::new(Symbol::qualified("ruby", "each"));

    let broken = ruby_break(
        &mut cx,
        &scope,
        Ref::Symbol(Symbol::qualified("ruby", "break-value")),
    )
    .unwrap();
    let next = ruby_next(
        &mut cx,
        &scope,
        Ref::Symbol(Symbol::qualified("ruby", "next-value")),
    )
    .unwrap();

    assert_eq!(
        control_result_status(&cx, broken.reference()).unwrap(),
        Some(control_aborted_status())
    );
    assert_eq!(
        control_result_status(&cx, next.reference()).unwrap(),
        Some(control_aborted_status())
    );
    assert!(
        ruby_dsl_profile()
            .organs
            .iter()
            .any(|organ| organ.organ == sim_lib_control::control_organ_symbol())
    );
}

#[test]
fn ruby_profile_publishes_honest_fidelity() {
    let mut cx = cx();
    let mut registry = ProfileRegistry::new();
    let profile = install_ruby_dsl_profile(&mut cx, &mut registry).unwrap();

    assert!(
        profile
            .organs
            .iter()
            .any(|organ| organ.organ == sim_lib_dispatch::dispatch_organ_symbol())
    );
    assert!(
        profile
            .fidelity_badges
            .iter()
            .any(|badge| badge.badge == ruby_blocks_fidelity_symbol() && badge.level == 0)
    );
    assert!(registry.profile(&profile.symbol).is_some());
}
