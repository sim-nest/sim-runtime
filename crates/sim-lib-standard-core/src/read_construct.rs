//! Read/construct support exposing profiles and badges as runtime objects.

use std::sync::Arc;

use sim_kernel::{
    AbiVersion, Args, CORE_CLASS_CLASS_ID, CORE_FUNCTION_CLASS_ID, Callable, Class, ClassId,
    ClassRef, Cx, DefaultFactory, Export, Expr, Factory, Lib, LibManifest, LibTarget, Linker,
    Object, ObjectCompat, ObjectEncode, ObjectEncoding, ReadConstructor, ReadConstructorRef, Ref,
    Result, ShapeRef, Symbol, TableRef, Value, Version,
};

use crate::{
    FidelityBadge, LanguageProfile, fidelity_badge_class_symbol, language_profile_class_symbol,
};

const PROFILE_CLASS_ID: ClassId = ClassId(6100);
const FIDELITY_BADGE_CLASS_ID: ClassId = ClassId(6101);

/// Runtime object wrapping a [`LanguageProfile`] as the `standard/Profile` class.
#[derive(Clone)]
pub struct LanguageProfileValue {
    profile: LanguageProfile,
}

impl LanguageProfileValue {
    /// Wrap `profile` as a runtime value.
    pub fn new(profile: LanguageProfile) -> Self {
        Self { profile }
    }

    /// The wrapped profile.
    pub fn profile(&self) -> &LanguageProfile {
        &self.profile
    }
}

impl Object for LanguageProfileValue {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<standard-profile {}>", self.profile.symbol))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for LanguageProfileValue {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        class_value_or_stub(cx, PROFILE_CLASS_ID, language_profile_class_symbol())
    }

    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Call {
            operator: Box::new(Expr::Symbol(language_profile_class_symbol())),
            args: self.profile.to_constructor_args(),
        })
    }

    fn as_object_encoder(&self) -> Option<&dyn ObjectEncode> {
        Some(self)
    }
}

impl ObjectEncode for LanguageProfileValue {
    fn object_encoding(&self, _cx: &mut Cx) -> Result<ObjectEncoding> {
        Ok(ObjectEncoding::Constructor {
            class: language_profile_class_symbol(),
            args: self.profile.to_constructor_args(),
        })
    }
}

impl sim_citizen::Citizen for LanguageProfileValue {
    fn citizen_symbol() -> Symbol {
        language_profile_class_symbol()
    }

    fn citizen_version() -> u32 {
        0
    }

    fn citizen_arity() -> usize {
        11
    }

    fn citizen_fields() -> &'static [&'static str] {
        &[
            "symbol",
            "reader",
            "lowering",
            "eval_policy",
            "organs",
            "backing_requirements",
            "numeric_tower",
            "capabilities",
            "unsupported_forms",
            "conformance_tests",
            "fidelity_badges",
        ]
    }
}

/// Runtime object wrapping a [`FidelityBadge`] as the `standard/FidelityBadge` class.
#[derive(Clone)]
pub struct FidelityBadgeValue {
    badge: FidelityBadge,
}

impl FidelityBadgeValue {
    /// Wrap `badge` as a runtime value.
    pub fn new(badge: FidelityBadge) -> Self {
        Self { badge }
    }

    /// The wrapped badge.
    pub fn badge(&self) -> &FidelityBadge {
        &self.badge
    }
}

impl Object for FidelityBadgeValue {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<standard-fidelity {}>", self.badge.badge))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for FidelityBadgeValue {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        class_value_or_stub(cx, FIDELITY_BADGE_CLASS_ID, fidelity_badge_class_symbol())
    }

    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Call {
            operator: Box::new(Expr::Symbol(fidelity_badge_class_symbol())),
            args: self.badge.to_constructor_args(),
        })
    }

    fn as_object_encoder(&self) -> Option<&dyn ObjectEncode> {
        Some(self)
    }
}

impl ObjectEncode for FidelityBadgeValue {
    fn object_encoding(&self, _cx: &mut Cx) -> Result<ObjectEncoding> {
        Ok(ObjectEncoding::Constructor {
            class: fidelity_badge_class_symbol(),
            args: self.badge.to_constructor_args(),
        })
    }
}

impl sim_citizen::Citizen for FidelityBadgeValue {
    fn citizen_symbol() -> Symbol {
        fidelity_badge_class_symbol()
    }

    fn citizen_version() -> u32 {
        0
    }

    fn citizen_arity() -> usize {
        4
    }

    fn citizen_fields() -> &'static [&'static str] {
        &["subject", "badge", "level", "evidence"]
    }
}

/// Register the `standard/Profile` and `standard/FidelityBadge` classes into the
/// registry, idempotently.
///
/// This installs the read/construct surface that lets the standard distribution
/// round-trip profiles and badges as runtime objects; classes already present
/// are left untouched.
pub fn install_standard_core_classes(cx: &mut Cx) -> Result<()> {
    sim_lib_core::install_once(cx, &StandardCoreClassesLib).map(|_| ())
}

/// Manifest id for the standard read/construct class lib.
pub fn standard_core_classes_lib_symbol() -> Symbol {
    Symbol::qualified("standard", "classes")
}

struct StandardCoreClassesLib;

impl Lib for StandardCoreClassesLib {
    fn manifest(&self) -> LibManifest {
        LibManifest {
            id: standard_core_classes_lib_symbol(),
            version: Version(env!("CARGO_PKG_VERSION").to_owned()),
            abi: AbiVersion { major: 0, minor: 1 },
            target: LibTarget::HostRegistered,
            requires: Vec::new(),
            capabilities: Vec::new(),
            exports: [StandardClassKind::Profile, StandardClassKind::FidelityBadge]
                .into_iter()
                .map(|kind| Export::Class {
                    symbol: kind.symbol(),
                    class_id: Some(kind.id()),
                })
                .collect(),
        }
    }

    fn load(&self, _cx: &mut sim_kernel::LoadCx, linker: &mut Linker<'_>) -> Result<()> {
        register_standard_class(linker, StandardClassKind::Profile)?;
        register_standard_class(linker, StandardClassKind::FidelityBadge)
    }
}

#[derive(Clone, Copy)]
enum StandardClassKind {
    Profile,
    FidelityBadge,
}

impl StandardClassKind {
    fn id(self) -> ClassId {
        match self {
            Self::Profile => PROFILE_CLASS_ID,
            Self::FidelityBadge => FIDELITY_BADGE_CLASS_ID,
        }
    }

    fn symbol(self) -> Symbol {
        match self {
            Self::Profile => language_profile_class_symbol(),
            Self::FidelityBadge => fidelity_badge_class_symbol(),
        }
    }

    fn display_name(self) -> &'static str {
        match self {
            Self::Profile => "standard/Profile",
            Self::FidelityBadge => "standard/FidelityBadge",
        }
    }
}

#[derive(Clone)]
struct StandardClass {
    kind: StandardClassKind,
}

impl Object for StandardClass {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<class {}>", self.kind.display_name()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for StandardClass {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        class_value_or_stub(cx, CORE_CLASS_CLASS_ID, Symbol::qualified("core", "Class"))
    }

    fn as_expr(&self, _cx: &mut Cx) -> Result<Expr> {
        Ok(Expr::Symbol(self.kind.symbol()))
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }

    fn as_class(&self) -> Option<&dyn Class> {
        Some(self)
    }
}

impl Callable for StandardClass {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        construct_standard_value(cx, self.kind, args.into_vec())
    }
}

impl Class for StandardClass {
    fn id(&self) -> ClassId {
        self.kind.id()
    }

    fn symbol(&self) -> Symbol {
        self.kind.symbol()
    }

    fn constructor_shape(&self, cx: &mut Cx) -> Result<ShapeRef> {
        cx.factory().nil()
    }

    fn instance_shape(&self, cx: &mut Cx) -> Result<ShapeRef> {
        cx.factory().nil()
    }

    fn read_constructor(&self, _cx: &mut Cx) -> Result<Option<ReadConstructorRef>> {
        Ok(Some(DefaultFactory.opaque(Arc::new(
            StandardReadConstructor { kind: self.kind },
        ))?))
    }

    fn members(&self, cx: &mut Cx) -> Result<TableRef> {
        cx.factory().table(Vec::new())
    }
}

#[derive(Clone)]
struct StandardReadConstructor {
    kind: StandardClassKind,
}

impl Object for StandardReadConstructor {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!("#<read-constructor {}>", self.kind.display_name()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for StandardReadConstructor {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        class_value_or_stub(
            cx,
            CORE_FUNCTION_CLASS_ID,
            Symbol::qualified("core", "Function"),
        )
    }

    fn as_read_constructor(&self) -> Option<&dyn ReadConstructor> {
        Some(self)
    }
}

impl ReadConstructor for StandardReadConstructor {
    fn symbol(&self) -> Symbol {
        self.kind.symbol()
    }

    fn args_shape(&self, cx: &mut Cx) -> Result<ShapeRef> {
        cx.factory().nil()
    }

    fn construct_read(&self, cx: &mut Cx, args: Vec<Value>) -> Result<Value> {
        construct_standard_value(cx, self.kind, args)
    }
}

fn construct_standard_value(
    cx: &mut Cx,
    kind: StandardClassKind,
    args: Vec<Value>,
) -> Result<Value> {
    let exprs = value_exprs(cx, args)?;
    match kind {
        StandardClassKind::Profile => cx.factory().opaque(Arc::new(LanguageProfileValue::new(
            LanguageProfile::from_constructor_args(exprs)?,
        ))),
        StandardClassKind::FidelityBadge => cx.factory().opaque(Arc::new(FidelityBadgeValue::new(
            FidelityBadge::from_constructor_args(exprs)?,
        ))),
    }
}

fn value_exprs(cx: &mut Cx, args: Vec<Value>) -> Result<Vec<Expr>> {
    let mut exprs = Vec::with_capacity(args.len());
    for value in args {
        exprs.push(value.object().as_expr(cx)?);
    }
    Ok(exprs)
}

fn register_standard_class(linker: &mut Linker<'_>, kind: StandardClassKind) -> Result<()> {
    let class = DefaultFactory
        .opaque(Arc::new(StandardClass { kind }))
        .expect("standard class should be boxable");
    linker.class_value(kind.symbol(), class)?;
    Ok(())
}

fn install_language_profile_citizen(linker: &mut Linker<'_>) -> Result<()> {
    register_standard_class(linker, StandardClassKind::Profile)
}

fn install_fidelity_badge_citizen(linker: &mut Linker<'_>) -> Result<()> {
    register_standard_class(linker, StandardClassKind::FidelityBadge)
}

fn conformance_language_profile_citizen(cx: &mut Cx) -> Result<()> {
    let profile = LanguageProfile::new(Symbol::qualified("standard-citizen", "profile"));
    let value = cx
        .factory()
        .opaque(Arc::new(LanguageProfileValue::new(profile)))?;
    sim_citizen::check_value_fixture(cx, value)
}

fn conformance_fidelity_badge_citizen(cx: &mut Cx) -> Result<()> {
    let badge = FidelityBadge::new(
        Ref::Symbol(Symbol::qualified("standard-citizen", "subject")),
        Symbol::qualified("standard-citizen", "badge"),
        2,
        Ref::Symbol(Symbol::qualified("standard-citizen", "evidence")),
    );
    let value = cx
        .factory()
        .opaque(Arc::new(FidelityBadgeValue::new(badge)))?;
    sim_citizen::check_value_fixture(cx, value)
}

sim_citizen::inventory::submit! {
    sim_citizen::CitizenInfo {
        symbol: "standard/Profile",
        version: 0,
        crate_name: env!("CARGO_PKG_NAME"),
        arity: 10,
        install: install_language_profile_citizen,
        conformance: conformance_language_profile_citizen,
    }
}

sim_citizen::inventory::submit! {
    sim_citizen::CitizenInfo {
        symbol: "standard/FidelityBadge",
        version: 0,
        crate_name: env!("CARGO_PKG_NAME"),
        arity: 4,
        install: install_fidelity_badge_citizen,
        conformance: conformance_fidelity_badge_citizen,
    }
}

fn class_value_or_stub(cx: &mut Cx, id: ClassId, symbol: Symbol) -> Result<Value> {
    if let Some(value) = cx.registry().class_by_symbol(&symbol) {
        return Ok(value.clone());
    }
    cx.factory().class_stub(id, symbol)
}
