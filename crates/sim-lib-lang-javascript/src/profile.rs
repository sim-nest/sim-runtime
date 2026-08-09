use sim_kernel::{Cx, Expr, Result, Symbol, Value};
use sim_lib_standard_core::{
    CoercionPolicy, GuestRuntimeKit, LanguageProfile, OrganUse, ProfileBackingLib, ProfileRegistry,
    TruthPolicy, install_language_profile,
};
use std::sync::Arc;

/// One intrinsic admitted by this phase's checked scalar core.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct JavascriptIntrinsic {
    /// ECMAScript name.
    pub name: &'static str,
    /// Composed implementation boundary.
    pub backing: &'static str,
}

/// Intrinsics admitted by the thin core; later phases extend this manifest.
pub const fn javascript_intrinsic_manifest() -> &'static [JavascriptIntrinsic] {
    &[
        JavascriptIntrinsic {
            name: "Number",
            backing: "sim-lib-numbers-f64",
        },
        JavascriptIntrinsic {
            name: "BigInt",
            backing: "sim-lib-numbers-i64",
        },
        JavascriptIntrinsic {
            name: "Boolean",
            backing: "javascript coercion policy",
        },
        JavascriptIntrinsic {
            name: "String",
            backing: "javascript coercion policy",
        },
    ]
}
/// Explicit unsupported surface for the current checked phase.
pub const fn javascript_gap_catalog() -> &'static [&'static str] {
    &[
        "compiler-or-bytecode",
        "foreign-engine-or-node",
        "general-realm-agent-engine",
        "host-event-loop",
        "weak-apis",
        "retain-policy-leaks-cycles",
        "proxy-invariants",
        "exotic-object-invariants",
    ]
}
/// Build the inspectable JavaScript language profile.
pub fn javascript_core_profile() -> LanguageProfile {
    let mut p = LanguageProfile::new(Symbol::qualified("lang", "javascript-core/v1"))
        .with_reader(Symbol::qualified("codec", "javascript"))
        .with_lowering(Symbol::qualified("javascript", "expr-lowering/v1"))
        .with_eval_policy(Symbol::qualified("javascript", "direct-eval/v1"))
        .with_organ(OrganUse::new(sim_lib_binding::binding_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_control::control_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_mutation::mutation_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_sequence::sequence_organ_symbol()))
        .with_organ(OrganUse::new(sim_lib_dispatch::dispatch_organ_symbol()))
        .requiring(sim_lib_mutation::standard_mutate_capability());
    for gap in javascript_gap_catalog() {
        p = p.with_unsupported_form(Symbol::qualified("javascript", *gap));
    }
    p
}
/// Install the profile and shared backing-organ declarations.
pub fn install_javascript_core_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
) -> Result<LanguageProfile> {
    install_language_profile(
        cx,
        registry,
        javascript_core_profile(),
        &[
            ProfileBackingLib::loadable(
                sim_lib_binding::binding_organ_symbol(),
                sim_lib_binding::manifest_name(),
                sim_lib_binding::install_binding_lib,
                Some(sim_lib_binding::publish_binding_organ_claims_for_lib),
            ),
            ProfileBackingLib::loadable(
                sim_lib_control::control_organ_symbol(),
                sim_lib_control::manifest_name(),
                sim_lib_control::install_control_lib,
                None,
            ),
            ProfileBackingLib::unresolved(
                sim_lib_mutation::mutation_organ_symbol(),
                Symbol::qualified("sim", "mutation"),
            ),
            ProfileBackingLib::loadable(
                sim_lib_sequence::sequence_organ_symbol(),
                sim_lib_sequence::manifest_name(),
                sim_lib_sequence::install_sequence_lib,
                Some(sim_lib_sequence::publish_sequence_organ_claims_for_lib),
            ),
            ProfileBackingLib::unresolved(
                sim_lib_dispatch::dispatch_organ_symbol(),
                Symbol::qualified("sim", "dispatch"),
            ),
        ],
        &[],
    )
}
struct JsTruth;
impl TruthPolicy for JsTruth {
    fn is_truthy(&self, cx: &mut Cx, value: &Value) -> Result<bool> {
        Ok(!matches!(
            value.object().as_expr(cx)?,
            Expr::Bool(false) | Expr::Nil
        ))
    }
}
struct JsCoercion;
impl CoercionPolicy for JsCoercion {
    fn to_number(&self, _: &mut Cx, _: &Value) -> Result<Option<Value>> {
        Ok(None)
    }
    fn to_string(&self, _: &mut Cx, _: &Value) -> Result<Option<Value>> {
        Ok(None)
    }
}
/// Build the runtime-kit registration used by the direct evaluator.
pub fn javascript_runtime_kit(cx: &mut Cx) -> Result<GuestRuntimeKit> {
    Ok(GuestRuntimeKit::new(
        Arc::new(JsTruth),
        Arc::new(JsCoercion),
        cx.factory().nil()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registration_is_complete() {
        let e = javascript_core_profile().checked_guest_evidence().unwrap();
        assert_eq!(e.organs.len(), 5);
        assert_eq!(e.capabilities.len(), 1);
        assert_eq!(e.gaps.len(), 8);
        assert_eq!(javascript_intrinsic_manifest().len(), 4);
    }
}
