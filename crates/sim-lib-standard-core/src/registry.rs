//! In-memory registry of installed language profiles.

use std::collections::BTreeMap;

use sim_kernel::{Error, Result, Symbol};

use crate::LanguageProfile;

/// In-memory map of installed [`LanguageProfile`]s keyed by profile symbol.
#[derive(Clone, Debug, Default)]
pub struct ProfileRegistry {
    profiles: BTreeMap<Symbol, LanguageProfile>,
}

impl ProfileRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register `profile`, failing if a profile with the same symbol is already
    /// installed.
    pub fn register_profile(&mut self, profile: LanguageProfile) -> Result<()> {
        let symbol = profile.symbol.clone();
        if self.profiles.contains_key(&symbol) {
            return Err(Error::DuplicateExport {
                kind: "standard-profile",
                symbol,
            });
        }
        self.profiles.insert(symbol, profile);
        Ok(())
    }

    /// Look up an installed profile by symbol.
    pub fn profile(&self, symbol: &Symbol) -> Option<&LanguageProfile> {
        self.profiles.get(symbol)
    }

    /// Iterate the installed profiles in symbol order.
    pub fn profiles(&self) -> impl Iterator<Item = &LanguageProfile> {
        self.profiles.values()
    }

    /// Number of installed profiles.
    pub fn len(&self) -> usize {
        self.profiles.len()
    }

    /// Whether no profiles are installed.
    pub fn is_empty(&self) -> bool {
        self.profiles.is_empty()
    }
}
