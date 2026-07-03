//! The `LanguageProfile` model: organ uses, badges, and profile metadata.

use sim_kernel::{CapabilityName, Expr, Ref, Symbol};

use crate::fidelity::{FidelityBadge, expr_kind, symbol_from_expr};

/// One organ a profile uses, with its configuration options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrganUse {
    /// Symbol of the organ being used.
    pub organ: Symbol,
    /// Key/value options configuring the organ.
    pub options: Vec<(Symbol, Expr)>,
}

impl OrganUse {
    /// Use `organ` with no options.
    pub fn new(organ: Symbol) -> Self {
        Self {
            organ,
            options: Vec::new(),
        }
    }

    /// Add an option `key`/`value` pair.
    pub fn with_option(mut self, key: Symbol, value: Expr) -> Self {
        self.options.push((key, value));
        self
    }

    /// Encode this organ use as an expression (organ symbol plus option map).
    pub fn to_expr(&self) -> Expr {
        Expr::List(vec![
            Expr::Symbol(self.organ.clone()),
            Expr::Map(
                self.options
                    .iter()
                    .map(|(key, value)| (Expr::Symbol(key.clone()), value.clone()))
                    .collect(),
            ),
        ])
    }

    /// Decode an organ use from its [`OrganUse::to_expr`] encoding.
    pub fn from_expr(expr: &Expr) -> sim_kernel::Result<Self> {
        let Expr::List(items) = expr else {
            return Err(sim_kernel::Error::TypeMismatch {
                expected: "organ-use list",
                found: expr_kind(expr),
            });
        };
        let [organ, options] = items.as_slice() else {
            return Err(sim_kernel::Error::Eval(
                "organ use expects organ symbol and option map".to_owned(),
            ));
        };
        let Expr::Map(entries) = options else {
            return Err(sim_kernel::Error::TypeMismatch {
                expected: "organ-use option map",
                found: expr_kind(options),
            });
        };
        Ok(Self {
            organ: symbol_from_expr(organ, "organ symbol")?,
            options: entries
                .iter()
                .map(|(key, value)| Ok((symbol_from_expr(key, "option symbol")?, value.clone())))
                .collect::<sim_kernel::Result<Vec<_>>>()?,
        })
    }
}

/// A language profile: the reader, lowering, eval-policy, organs, and metadata
/// that present one surface language over the shared `Expr` graph.
///
/// Profiles are the unit the standard distribution installs, diffs, and tests;
/// the per-language `sim-lib-lang-*` crates build one each.
///
/// # Examples
///
/// ```
/// use sim_kernel::Symbol;
/// use sim_lib_standard_core::{LanguageProfile, OrganUse, standard_control_organ_symbol};
///
/// let profile = LanguageProfile::new(Symbol::qualified("lang", "demo/v1"))
///     .with_reader(Symbol::qualified("codec", "lisp"))
///     .with_eval_policy(Symbol::qualified("eval", "default"))
///     .with_organ(OrganUse::new(standard_control_organ_symbol()));
///
/// assert_eq!(profile.reader, Symbol::qualified("codec", "lisp"));
/// assert_eq!(profile.organs.len(), 1);
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LanguageProfile {
    /// Symbol naming the profile.
    pub symbol: Symbol,
    /// Reader (codec) symbol the profile parses with.
    pub reader: Symbol,
    /// Lowering symbol mapping surface forms onto the shared graph.
    pub lowering: Symbol,
    /// Eval-policy symbol the profile evaluates under.
    pub eval_policy: Symbol,
    /// Organs the profile uses.
    pub organs: Vec<OrganUse>,
    /// Optional numeric tower symbol.
    pub numeric_tower: Option<Symbol>,
    /// Capabilities the profile requires.
    pub capabilities: Vec<CapabilityName>,
    /// Surface forms the profile does not support.
    pub unsupported_forms: Vec<Symbol>,
    /// Conformance tests covering the profile.
    pub conformance_tests: Vec<Symbol>,
    /// Fidelity badges declared for the profile.
    pub fidelity_badges: Vec<FidelityBadge>,
}

impl LanguageProfile {
    /// Start a profile named `symbol` with unspecified reader/lowering/eval-policy
    /// and no organs.
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            reader: unspecified_symbol("reader"),
            lowering: unspecified_symbol("lowering"),
            eval_policy: unspecified_symbol("eval-policy"),
            organs: Vec::new(),
            numeric_tower: None,
            capabilities: Vec::new(),
            unsupported_forms: Vec::new(),
            conformance_tests: Vec::new(),
            fidelity_badges: Vec::new(),
        }
    }

    /// Set the reader symbol.
    pub fn with_reader(mut self, reader: Symbol) -> Self {
        self.reader = reader;
        self
    }

    /// Set the lowering symbol.
    pub fn with_lowering(mut self, lowering: Symbol) -> Self {
        self.lowering = lowering;
        self
    }

    /// Set the eval-policy symbol.
    pub fn with_eval_policy(mut self, eval_policy: Symbol) -> Self {
        self.eval_policy = eval_policy;
        self
    }

    /// Add an organ use.
    pub fn with_organ(mut self, organ: OrganUse) -> Self {
        self.organs.push(organ);
        self
    }

    /// Set the numeric tower symbol.
    pub fn with_numeric_tower(mut self, numeric_tower: Symbol) -> Self {
        self.numeric_tower = Some(numeric_tower);
        self
    }

    /// Add a required capability.
    pub fn requiring(mut self, capability: CapabilityName) -> Self {
        self.capabilities.push(capability);
        self
    }

    /// Add an unsupported surface form.
    pub fn with_unsupported_form(mut self, form: Symbol) -> Self {
        self.unsupported_forms.push(form);
        self
    }

    /// Add a conformance test.
    pub fn with_conformance_test(mut self, test: Symbol) -> Self {
        self.conformance_tests.push(test);
        self
    }

    /// Add a fidelity badge.
    pub fn with_fidelity_badge(mut self, badge: FidelityBadge) -> Self {
        self.fidelity_badges.push(badge);
        self
    }

    /// Encode this profile as constructor arguments for the `standard/Profile` class.
    pub fn to_constructor_args(&self) -> Vec<Expr> {
        vec![
            Expr::Symbol(self.symbol.clone()),
            Expr::Symbol(self.reader.clone()),
            Expr::Symbol(self.lowering.clone()),
            Expr::Symbol(self.eval_policy.clone()),
            Expr::List(self.organs.iter().map(OrganUse::to_expr).collect()),
            self.numeric_tower
                .clone()
                .map(Expr::Symbol)
                .unwrap_or(Expr::Nil),
            Expr::List(
                self.capabilities
                    .iter()
                    .map(|capability| Expr::String(capability.as_str().to_owned()))
                    .collect(),
            ),
            Expr::List(
                self.unsupported_forms
                    .iter()
                    .cloned()
                    .map(Expr::Symbol)
                    .collect(),
            ),
            Expr::List(
                self.conformance_tests
                    .iter()
                    .cloned()
                    .map(Expr::Symbol)
                    .collect(),
            ),
            Expr::List(
                self.fidelity_badges
                    .iter()
                    .map(|badge| Expr::Call {
                        operator: Box::new(Expr::Symbol(crate::fidelity_badge_class_symbol())),
                        args: badge.to_constructor_args(),
                    })
                    .collect(),
            ),
        ]
    }

    /// Decode a profile from `standard/Profile` constructor arguments.
    pub fn from_constructor_args(args: Vec<Expr>) -> sim_kernel::Result<Self> {
        let [
            symbol,
            reader,
            lowering,
            eval_policy,
            organs,
            numeric_tower,
            capabilities,
            unsupported_forms,
            conformance_tests,
            fidelity_badges,
        ] = args.as_slice()
        else {
            return Err(sim_kernel::Error::Eval(
                "standard/Profile expects ten constructor arguments".to_owned(),
            ));
        };

        Ok(Self {
            symbol: symbol_from_expr(symbol, "profile symbol")?,
            reader: symbol_from_expr(reader, "reader symbol")?,
            lowering: symbol_from_expr(lowering, "lowering symbol")?,
            eval_policy: symbol_from_expr(eval_policy, "eval policy symbol")?,
            organs: organ_uses_from_expr(organs)?,
            numeric_tower: optional_symbol(numeric_tower)?,
            capabilities: capabilities_from_expr(capabilities)?,
            unsupported_forms: symbols_from_expr(unsupported_forms, "unsupported form")?,
            conformance_tests: symbols_from_expr(conformance_tests, "conformance test")?,
            fidelity_badges: badges_from_expr(fidelity_badges)?,
        })
    }
}

/// Class symbol for the `standard/Profile` runtime object.
pub fn language_profile_class_symbol() -> Symbol {
    Symbol::qualified("standard", "Profile")
}

/// Symbol naming the built-in sim-expression profile.
pub fn sim_expression_profile_symbol() -> Symbol {
    Symbol::qualified("lang", "sim-expression/v1")
}

/// The built-in sim-expression profile: the standard distribution's own surface
/// over the shared `Expr` graph (lisp reader, default eval policy, core organs).
pub fn sim_expression_profile() -> LanguageProfile {
    let profile = sim_expression_profile_symbol();
    let test = Symbol::qualified("test", "sim-expression-core");
    LanguageProfile::new(profile.clone())
        .with_reader(Symbol::qualified("codec", "lisp"))
        .with_lowering(Symbol::qualified("standard", "identity-lowering"))
        .with_eval_policy(Symbol::qualified("eval", "default"))
        .with_organ(OrganUse::new(standard_control_organ_symbol()))
        .with_organ(OrganUse::new(standard_binding_organ_symbol()))
        .with_organ(OrganUse::new(standard_sequence_organ_symbol()))
        .with_organ(OrganUse::new(standard_pattern_organ_symbol()))
        .with_numeric_tower(Symbol::qualified("numbers", "sim-expression"))
        .with_conformance_test(test.clone())
        .with_fidelity_badge(FidelityBadge::new(
            Ref::Symbol(profile),
            Symbol::qualified("standard", "host-native"),
            1,
            Ref::Symbol(test),
        ))
}

/// Symbol for the standard control organ.
pub fn standard_control_organ_symbol() -> Symbol {
    Symbol::qualified("organ", "control")
}

/// Symbol for the standard binding organ.
pub fn standard_binding_organ_symbol() -> Symbol {
    Symbol::qualified("organ", "binding")
}

/// Symbol for the standard sequence organ.
pub fn standard_sequence_organ_symbol() -> Symbol {
    Symbol::qualified("organ", "sequence")
}

/// Symbol for the standard pattern organ.
pub fn standard_pattern_organ_symbol() -> Symbol {
    Symbol::qualified("organ", "pattern")
}

fn unspecified_symbol(name: &str) -> Symbol {
    Symbol::qualified("standard/unspecified", name.to_owned())
}

fn optional_symbol(expr: &Expr) -> sim_kernel::Result<Option<Symbol>> {
    match expr {
        Expr::Nil => Ok(None),
        Expr::Symbol(symbol) => Ok(Some(symbol.clone())),
        _ => Err(sim_kernel::Error::TypeMismatch {
            expected: "optional symbol",
            found: expr_kind(expr),
        }),
    }
}

fn organ_uses_from_expr(expr: &Expr) -> sim_kernel::Result<Vec<OrganUse>> {
    let Expr::List(items) = expr else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "organ-use list",
            found: expr_kind(expr),
        });
    };
    items.iter().map(OrganUse::from_expr).collect()
}

fn capabilities_from_expr(expr: &Expr) -> sim_kernel::Result<Vec<CapabilityName>> {
    let Expr::List(items) = expr else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "capability list",
            found: expr_kind(expr),
        });
    };
    items
        .iter()
        .map(|item| match item {
            Expr::String(name) => Ok(CapabilityName::new(name.clone())),
            _ => Err(sim_kernel::Error::TypeMismatch {
                expected: "capability string",
                found: expr_kind(item),
            }),
        })
        .collect()
}

fn symbols_from_expr(expr: &Expr, expected: &'static str) -> sim_kernel::Result<Vec<Symbol>> {
    let Expr::List(items) = expr else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "symbol list",
            found: expr_kind(expr),
        });
    };
    items
        .iter()
        .map(|item| symbol_from_expr(item, expected))
        .collect()
}

fn badges_from_expr(expr: &Expr) -> sim_kernel::Result<Vec<FidelityBadge>> {
    let Expr::List(items) = expr else {
        return Err(sim_kernel::Error::TypeMismatch {
            expected: "fidelity badge list",
            found: expr_kind(expr),
        });
    };
    items
        .iter()
        .map(|item| match item {
            Expr::Call { operator, args } => {
                let class = symbol_from_expr(operator, "badge class")?;
                if class != crate::fidelity_badge_class_symbol() {
                    return Err(sim_kernel::Error::Eval(format!(
                        "expected standard/FidelityBadge, found {class}"
                    )));
                }
                FidelityBadge::from_constructor_args(args.clone())
            }
            _ => Err(sim_kernel::Error::TypeMismatch {
                expected: "fidelity badge constructor call",
                found: expr_kind(item),
            }),
        })
        .collect()
}
