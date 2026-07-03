use sim_kernel::{Error, Expr, Result, Symbol};

/// How a language profile scopes its bindings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingScopeMode {
    /// Lexical scope (the default): bindings follow textual nesting.
    Lexical,
    /// Dynamic scope: bindings follow the dynamic call extent.
    Dynamic,
    /// Hybrid scope: both lexical and dynamic bindings coexist.
    Hybrid,
}

/// How a language profile treats macro hygiene.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HygieneMode {
    /// Hygienic (the default): introduced names cannot capture.
    Hygienic,
    /// Explicit: hygiene is opt-in per identifier.
    Explicit,
    /// Unhygienic: introduced names may capture freely.
    Unhygienic,
}

/// The binding and hygiene modes selected for a language profile.
///
/// Parsed from profile options and consumed by the `profile-modes` form to
/// configure how a profile's surface binds names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BindingProfileModes {
    /// The binding scope mode.
    pub scope: BindingScopeMode,
    /// The macro hygiene mode.
    pub hygiene: HygieneMode,
}

impl Default for BindingProfileModes {
    fn default() -> Self {
        Self {
            scope: BindingScopeMode::Lexical,
            hygiene: HygieneMode::Hygienic,
        }
    }
}

impl BindingProfileModes {
    /// Derives modes from profile option pairs, defaulting unset modes.
    ///
    /// Recognizes `scope`/`binding` keys for [`BindingScopeMode`] and
    /// `hygiene` for [`HygieneMode`]; other keys are ignored. Errors on an
    /// unknown mode symbol.
    ///
    /// # Examples
    ///
    /// ```
    /// use sim_kernel::{Expr, Symbol};
    /// use sim_lib_binding::{BindingProfileModes, BindingScopeMode, HygieneMode};
    ///
    /// let modes = BindingProfileModes::from_options(&[
    ///     (Symbol::new("scope"), Expr::Symbol(Symbol::new("dynamic"))),
    ///     (Symbol::new("hygiene"), Expr::Symbol(Symbol::new("explicit"))),
    /// ])
    /// .unwrap();
    /// assert_eq!(modes.scope, BindingScopeMode::Dynamic);
    /// assert_eq!(modes.hygiene, HygieneMode::Explicit);
    /// ```
    pub fn from_options(options: &[(Symbol, Expr)]) -> Result<Self> {
        let mut modes = Self::default();
        for (key, value) in options {
            if key_matches(key, "scope") || key_matches(key, "binding") {
                modes.scope = parse_scope_mode(value)?;
            } else if key_matches(key, "hygiene") {
                modes.hygiene = parse_hygiene_mode(value)?;
            }
        }
        Ok(modes)
    }
}

fn parse_scope_mode(expr: &Expr) -> Result<BindingScopeMode> {
    let symbol = symbol_value(expr, "binding scope mode")?;
    match symbol.name.as_ref() {
        "lexical" => Ok(BindingScopeMode::Lexical),
        "dynamic" => Ok(BindingScopeMode::Dynamic),
        "hybrid" => Ok(BindingScopeMode::Hybrid),
        _ => Err(Error::Eval(format!(
            "unsupported binding scope mode {symbol}"
        ))),
    }
}

fn parse_hygiene_mode(expr: &Expr) -> Result<HygieneMode> {
    let symbol = symbol_value(expr, "binding hygiene mode")?;
    match symbol.name.as_ref() {
        "hygienic" => Ok(HygieneMode::Hygienic),
        "explicit" => Ok(HygieneMode::Explicit),
        "unhygienic" => Ok(HygieneMode::Unhygienic),
        _ => Err(Error::Eval(format!(
            "unsupported binding hygiene mode {symbol}"
        ))),
    }
}

fn symbol_value<'a>(expr: &'a Expr, expected: &'static str) -> Result<&'a Symbol> {
    match expr {
        Expr::Symbol(symbol) => Ok(symbol),
        _ => Err(Error::TypeMismatch {
            expected,
            found: "non-symbol",
        }),
    }
}

fn key_matches(symbol: &Symbol, name: &str) -> bool {
    symbol.name.as_ref() == name
}
