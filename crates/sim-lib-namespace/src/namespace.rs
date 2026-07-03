//! The namespace value: a scope of symbol bindings with exports and imports.
//!
//! The kernel supplies the [`Symbol`] and [`Diagnostic`] contracts; this module
//! supplies the concrete namespace organ behavior -- interning local bindings,
//! marking exports, and importing exported bindings (with optional rename and
//! shadow control) from one namespace into another. See the crate
//! [`README`](https://docs.rs/sim-runtime) for the constellation framing.

use std::collections::{BTreeMap, BTreeSet};

use sim_kernel::{Diagnostic, Error, Result, Severity, Symbol};

/// Whether a [`Namespace`] is a package or a module scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamespaceKind {
    /// A package: a top-level namespace that other namespaces import from.
    Package,
    /// A module: a nested scope that imports and re-resolves bindings.
    Module,
}

/// Where a [`NamespaceEntry`] binding came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NamespaceBindingSource {
    /// Defined directly in this namespace.
    Local,
    /// Imported from another namespace's export.
    Import {
        /// The source namespace symbol the binding was imported from.
        namespace: Symbol,
        /// The exported name in the source namespace.
        exported: Symbol,
    },
}

/// One binding in a [`Namespace`]: a name, its resolution target, and its source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NamespaceEntry {
    name: Symbol,
    target: Symbol,
    source: NamespaceBindingSource,
}

impl NamespaceEntry {
    /// Construct an entry binding `name` to `target` with the given `source`.
    pub fn new(name: Symbol, target: Symbol, source: NamespaceBindingSource) -> Self {
        Self {
            name,
            target,
            source,
        }
    }

    /// The name this binding is reached by within its namespace.
    pub fn name(&self) -> &Symbol {
        &self.name
    }

    /// The symbol this binding resolves to.
    pub fn target(&self) -> &Symbol {
        &self.target
    }

    /// Whether this binding is local or imported, and from where.
    pub fn source(&self) -> &NamespaceBindingSource {
        &self.source
    }

    fn imported_as(&self, alias: Symbol, namespace: Symbol, exported: Symbol) -> Self {
        Self {
            name: alias,
            target: self.target.clone(),
            source: NamespaceBindingSource::Import {
                namespace,
                exported,
            },
        }
    }
}

/// Options controlling a single [`Namespace::import_from`] call.
///
/// Built fluently from [`ImportOptions::new`]; defaults to no rename and no
/// shadowing (an import over an existing binding is a conflict).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportOptions {
    rename: Option<Symbol>,
    allow_shadow: bool,
}

impl ImportOptions {
    /// Default options: import under the exported name, reject shadowing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Import the binding under `name` instead of its exported name.
    pub fn rename(mut self, name: Symbol) -> Self {
        self.rename = Some(name);
        self
    }

    /// Allow the import to overwrite an existing binding of the same name.
    pub fn allow_shadow(mut self) -> Self {
        self.allow_shadow = true;
        self
    }
}

/// A named scope of symbol bindings: the core value of the namespace organ.
///
/// A namespace holds local and imported [`NamespaceEntry`] bindings, a set of
/// exported names, and any [`Diagnostic`]s raised while building it. Bindings
/// are ordered by symbol so resolution and iteration are deterministic. Imports
/// flow from one namespace's exports into another via
/// [`Namespace::import_from`].
///
/// # Examples
///
/// ```
/// use sim_kernel::Symbol;
/// use sim_lib_namespace::{ImportOptions, Namespace, NamespaceBindingSource};
///
/// let mut source = Namespace::package(Symbol::qualified("pkg", "sequence"));
/// source
///     .define(Symbol::new("map"), Symbol::qualified("sequence", "map.v1"))
///     .unwrap();
/// source.export(Symbol::new("map")).unwrap();
///
/// let mut user = Namespace::module(Symbol::qualified("module", "user"));
/// user.import_from(
///     &source,
///     &Symbol::new("map"),
///     ImportOptions::new().rename(Symbol::new("seq-map")),
/// )
/// .unwrap();
///
/// let entry = user.resolve(&Symbol::new("seq-map")).unwrap();
/// assert_eq!(entry.target(), &Symbol::qualified("sequence", "map.v1"));
/// assert_eq!(
///     entry.source(),
///     &NamespaceBindingSource::Import {
///         namespace: Symbol::qualified("pkg", "sequence"),
///         exported: Symbol::new("map"),
///     }
/// );
/// ```
#[derive(Clone, Debug)]
pub struct Namespace {
    symbol: Symbol,
    kind: NamespaceKind,
    bindings: BTreeMap<Symbol, NamespaceEntry>,
    exports: BTreeSet<Symbol>,
    diagnostics: Vec<Diagnostic>,
}

impl Namespace {
    /// Create an empty package namespace named `symbol`.
    pub fn package(symbol: Symbol) -> Self {
        Self::new(symbol, NamespaceKind::Package)
    }

    /// Create an empty module namespace named `symbol`.
    pub fn module(symbol: Symbol) -> Self {
        Self::new(symbol, NamespaceKind::Module)
    }

    /// Create an empty namespace named `symbol` of the given `kind`.
    pub fn new(symbol: Symbol, kind: NamespaceKind) -> Self {
        Self {
            symbol,
            kind,
            bindings: BTreeMap::new(),
            exports: BTreeSet::new(),
            diagnostics: Vec::new(),
        }
    }

    /// The symbol that names this namespace.
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }

    /// Whether this namespace is a package or a module.
    pub fn kind(&self) -> NamespaceKind {
        self.kind
    }

    /// Diagnostics accumulated while building this namespace (e.g. shadow conflicts).
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// Define a local binding from `name` to `target` in this namespace.
    ///
    /// # Errors
    ///
    /// Returns an error and records a diagnostic if `name` is already bound.
    pub fn define(&mut self, name: Symbol, target: Symbol) -> Result<()> {
        self.insert_binding(
            name.clone(),
            NamespaceEntry::new(name, target, NamespaceBindingSource::Local),
            false,
        )
    }

    /// Mark an existing binding `name` as exported so others may import it.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownSymbol`](sim_kernel::Error::UnknownSymbol) if
    /// `name` is not bound in this namespace.
    pub fn export(&mut self, name: Symbol) -> Result<()> {
        if !self.bindings.contains_key(&name) {
            return Err(Error::UnknownSymbol { symbol: name });
        }
        self.exports.insert(name);
        Ok(())
    }

    /// Import an exported binding from `source` into this namespace.
    ///
    /// The imported entry keeps the source's resolution target but records an
    /// [`NamespaceBindingSource::Import`] origin. `options` may rename the
    /// binding or allow it to shadow an existing one.
    ///
    /// # Errors
    ///
    /// Returns an error if `exported` is not exported by `source`, or if the
    /// destination name is already bound and shadowing was not allowed.
    pub fn import_from(
        &mut self,
        source: &Namespace,
        exported: &Symbol,
        options: ImportOptions,
    ) -> Result<()> {
        let entry = source.exported_entry(exported)?;
        let alias = options.rename.unwrap_or_else(|| exported.clone());
        let imported = entry.imported_as(alias.clone(), source.symbol.clone(), exported.clone());
        self.insert_binding(alias, imported, options.allow_shadow)
    }

    /// Look up the binding for `name`, returning `None` if it is unbound.
    pub fn resolve(&self, name: &Symbol) -> Option<&NamespaceEntry> {
        self.bindings.get(name)
    }

    /// Look up the binding for `name`, requiring that it is also exported.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnknownSymbol`](sim_kernel::Error::UnknownSymbol) if
    /// `name` is not exported (or not bound) by this namespace.
    pub fn exported_entry(&self, name: &Symbol) -> Result<&NamespaceEntry> {
        if !self.exports.contains(name) {
            return Err(Error::UnknownSymbol {
                symbol: name.clone(),
            });
        }
        self.bindings.get(name).ok_or_else(|| Error::UnknownSymbol {
            symbol: name.clone(),
        })
    }

    fn insert_binding(
        &mut self,
        name: Symbol,
        entry: NamespaceEntry,
        allow_shadow: bool,
    ) -> Result<()> {
        if self.bindings.contains_key(&name) && !allow_shadow {
            let diagnostic = shadow_conflict_diagnostic(&self.symbol, &name);
            let message = diagnostic.message.clone();
            self.diagnostics.push(diagnostic);
            return Err(Error::Eval(message));
        }
        self.bindings.insert(name, entry);
        Ok(())
    }
}

/// The diagnostic code attached to a namespace shadow-conflict diagnostic.
pub fn namespace_shadow_conflict_symbol() -> Symbol {
    Symbol::qualified("namespace", "shadow-conflict")
}

fn shadow_conflict_diagnostic(namespace: &Symbol, name: &Symbol) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        message: format!("namespace {namespace} shadow conflict for {name}"),
        source: None,
        span: None,
        code: Some(namespace_shadow_conflict_symbol()),
        related: Vec::new(),
    }
}
