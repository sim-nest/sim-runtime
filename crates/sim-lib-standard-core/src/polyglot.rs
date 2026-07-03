//! Polyglot profile functions callable across language profiles.

use std::{collections::BTreeMap, sync::Arc};

use sim_kernel::{
    Args, CORE_FUNCTION_CLASS_ID, Callable, ClassRef, Cx, Error, Object, ObjectCompat, Result,
    Symbol, Value,
};

use crate::{LanguageProfile, ProfileRegistry};

/// Body of a [`ProfileFunction`]: a callable closure over `Cx` and `Args`.
pub type ProfileFunctionBody = Arc<dyn Fn(&mut Cx, Args) -> Result<Value> + Send + Sync>;

/// A callable runtime object owned by a profile and scoped to an organ.
#[derive(Clone)]
pub struct ProfileFunction {
    defining_profile: Symbol,
    organ: Symbol,
    function: Symbol,
    body: ProfileFunctionBody,
}

impl ProfileFunction {
    /// Build a profile function for `function` in `organ`, defined by
    /// `defining_profile`, calling `body`.
    pub fn new<F>(defining_profile: Symbol, organ: Symbol, function: Symbol, body: F) -> Self
    where
        F: Fn(&mut Cx, Args) -> Result<Value> + Send + Sync + 'static,
    {
        Self {
            defining_profile,
            organ,
            function,
            body: Arc::new(body),
        }
    }

    /// Symbol of the profile that defined the function.
    pub fn defining_profile(&self) -> &Symbol {
        &self.defining_profile
    }

    /// Symbol of the organ the function belongs to.
    pub fn organ(&self) -> &Symbol {
        &self.organ
    }

    /// Symbol naming the function.
    pub fn function(&self) -> &Symbol {
        &self.function
    }
}

impl Callable for ProfileFunction {
    fn call(&self, cx: &mut Cx, args: Args) -> Result<Value> {
        (self.body)(cx, args)
    }
}

impl Object for ProfileFunction {
    fn display(&self, _cx: &mut Cx) -> Result<String> {
        Ok(format!(
            "#<profile-function {} defined-by {}>",
            self.function, self.defining_profile
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ObjectCompat for ProfileFunction {
    fn class(&self, cx: &mut Cx) -> Result<ClassRef> {
        cx.factory().class_stub(
            CORE_FUNCTION_CLASS_ID,
            Symbol::qualified("core", "Function"),
        )
    }

    fn as_callable(&self) -> Option<&dyn Callable> {
        Some(self)
    }
}

/// A function registered in a [`SharedOrganRuntime`], with its owning profile,
/// organ, name, and callable value.
#[derive(Clone, Debug)]
pub struct ProfileFunctionBinding {
    /// Symbol of the profile that defined the function.
    pub defining_profile: Symbol,
    /// Organ the function belongs to.
    pub organ: Symbol,
    /// Symbol naming the function.
    pub function: Symbol,
    /// The callable value.
    pub value: Value,
}

/// Runtime sharing organ functions across profiles: profiles may call a
/// function defined by another profile only when both use the function's organ.
#[derive(Clone, Debug, Default)]
pub struct SharedOrganRuntime {
    registry: ProfileRegistry,
    functions: BTreeMap<Symbol, ProfileFunctionBinding>,
}

impl SharedOrganRuntime {
    /// Create an empty runtime.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a profile so its organs and functions become available.
    pub fn register_profile(&mut self, profile: LanguageProfile) -> Result<()> {
        self.registry.register_profile(profile)
    }

    /// Look up a registered profile by symbol.
    pub fn profile(&self, symbol: &Symbol) -> Option<&LanguageProfile> {
        self.registry.profile(symbol)
    }

    /// Iterate the registered profiles.
    pub fn profiles(&self) -> impl Iterator<Item = &LanguageProfile> {
        self.registry.profiles()
    }

    /// Define a callable `function` in `organ`, attributed to `defining_profile`.
    ///
    /// Fails if the profile does not use the organ, the value is not callable, or
    /// the function name is already defined.
    pub fn define_function(
        &mut self,
        defining_profile: &Symbol,
        organ: Symbol,
        function: Symbol,
        value: Value,
    ) -> Result<()> {
        self.require_profile_uses_organ(defining_profile, &organ)?;
        if value.object().as_callable().is_none() {
            return Err(Error::TypeMismatch {
                expected: "callable",
                found: "non-callable",
            });
        }
        if self.functions.contains_key(&function) {
            return Err(Error::DuplicateExport {
                kind: "standard-profile-function",
                symbol: function,
            });
        }
        self.functions.insert(
            function.clone(),
            ProfileFunctionBinding {
                defining_profile: defining_profile.clone(),
                organ,
                function,
                value,
            },
        );
        Ok(())
    }

    /// Look up a defined function by symbol.
    pub fn function(&self, function: &Symbol) -> Option<&ProfileFunctionBinding> {
        self.functions.get(function)
    }

    /// Call `function` on behalf of `calling_profile`, requiring that profile to
    /// also use the function's organ.
    pub fn call_function(
        &self,
        cx: &mut Cx,
        calling_profile: &Symbol,
        function: &Symbol,
        args: Vec<Value>,
    ) -> Result<Value> {
        let binding = self
            .functions
            .get(function)
            .ok_or_else(|| Error::UnknownFunction {
                function: function.clone(),
            })?;
        self.require_profile_uses_organ(calling_profile, &binding.organ)?;
        let callable = binding
            .value
            .object()
            .as_callable()
            .ok_or(Error::TypeMismatch {
                expected: "callable",
                found: "non-callable",
            })?;
        callable.call(cx, Args::new(args))
    }

    fn require_profile_uses_organ(&self, profile: &Symbol, organ: &Symbol) -> Result<()> {
        let profile_record =
            self.registry
                .profile(profile)
                .ok_or_else(|| Error::UnknownSymbol {
                    symbol: profile.clone(),
                })?;
        if profile_record
            .organs
            .iter()
            .any(|used| &used.organ == organ)
        {
            Ok(())
        } else {
            Err(Error::Eval(format!(
                "profile {profile} does not use organ {organ}"
            )))
        }
    }
}

/// Wrap `body` as a callable [`ProfileFunction`] runtime value.
pub fn profile_function_value<F>(
    cx: &mut Cx,
    defining_profile: Symbol,
    organ: Symbol,
    function: Symbol,
    body: F,
) -> Result<Value>
where
    F: Fn(&mut Cx, Args) -> Result<Value> + Send + Sync + 'static,
{
    cx.factory().opaque(Arc::new(ProfileFunction::new(
        defining_profile,
        organ,
        function,
        body,
    )))
}
