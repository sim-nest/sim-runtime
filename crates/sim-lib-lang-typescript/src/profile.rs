use sim_kernel::{Cx, Result, Symbol};
use sim_lib_standard_core::{LanguageProfile, ProfileRegistry, install_language_profile};

/// Stable discovery id for the notation-only profile.
pub fn typescript_profile_symbol() -> Symbol {
    Symbol::qualified("language", "typescript-notation")
}

/// Explicit gaps which keep this crate below the compiler boundary.
pub const fn typescript_gap_manifest() -> &'static [&'static str] {
    &[
        "binding-and-scopes",
        "inference",
        "narrowing",
        "assignability",
        "source-preflight",
        "compiler-diagnostics",
        "code-producing-syntax",
        "checker-dependent-types",
        "emit-and-projects",
        "runtime-type-guards",
    ]
}

/// Build the single TypeScript notation profile.
pub fn typescript_notation_profile() -> LanguageProfile {
    let javascript = sim_lib_lang_javascript::javascript_core_profile();
    let mut profile = LanguageProfile::new(typescript_profile_symbol())
        .with_reader(Symbol::qualified("codec", "typescript"))
        .with_lowering(Symbol::qualified(
            "typescript",
            "direct-javascript-erasure/v1",
        ))
        .with_eval_policy(javascript.eval_policy.clone());
    for organ in javascript.organs {
        profile = profile.with_organ(organ);
    }
    for capability in javascript.capabilities {
        profile = profile.requiring(capability);
    }
    for gap in typescript_gap_manifest() {
        profile = profile.with_unsupported_form(Symbol::qualified("typescript", *gap));
    }
    profile
}

/// Install the profile; all runtime behavior remains in the JavaScript backing profile.
pub fn install_typescript_notation_profile(
    cx: &mut Cx,
    registry: &mut ProfileRegistry,
) -> Result<LanguageProfile> {
    install_language_profile(cx, registry, typescript_notation_profile(), &[], &[])
}
