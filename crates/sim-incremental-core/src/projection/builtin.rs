use std::{collections::BTreeSet, sync::Arc};

use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use sim_kernel::{ContentId, Datum, Symbol};

use super::{
    PackageIdentity, ProjectionError, ProjectionInputs, ProjectionKindRef, ProjectionOutput,
    ProjectionProvider, ProjectionRegistry,
};

/// Baseline open projection kinds required for source and release reasoning.
pub const BASELINE_PROJECTION_KINDS: &[&str] = &[
    "world/path-set-v1",
    "world/manifest-dependencies-v1",
    "world/public-api-v1",
    "world/exact-command-environment-v1",
    "world/generated-ownership-v1",
    "world/package-assembly-v1",
    "world/git-refs-v1",
    "world/index-routes-v1",
    "world/external-release-facts-v1",
    "no-v3/disclosure-policy-v1",
];

/// Canonical include/glob/ignore semantics for logical world paths.
///
/// Paths use relative `/`-separated logical names. Absolute paths, parent
/// traversal, backslashes, empty components, and `.` components are refused so
/// host path spelling cannot enter semantic identity. Includes form a union;
/// ignores subtract from it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathSelectionRules {
    includes: Vec<String>,
    ignores: Vec<String>,
}

impl PathSelectionRules {
    /// Checks and canonicalizes an explicit ordered rule set.
    pub fn new(
        includes: impl IntoIterator<Item = String>,
        ignores: impl IntoIterator<Item = String>,
    ) -> Result<Self, ProjectionError> {
        let includes = includes.into_iter().collect::<Vec<_>>();
        let ignores = ignores.into_iter().collect::<Vec<_>>();
        if includes.is_empty() {
            return Err(ProjectionError::InvalidPathSelection(
                "at least one include glob is required".to_owned(),
            ));
        }
        compile_globs(&includes)?;
        compile_globs(&ignores)?;
        Ok(Self { includes, ignores })
    }

    /// Selects exact logical path fact identities in canonical order.
    pub fn select(
        &self,
        paths: impl IntoIterator<Item = String>,
    ) -> Result<BTreeSet<super::FactId>, ProjectionError> {
        let includes = compile_globs(&self.includes)?;
        let ignores = compile_globs(&self.ignores)?;
        let mut selected = BTreeSet::new();
        for path in paths {
            validate_logical_path(&path)?;
            if includes.is_match(&path) && !ignores.is_match(&path) {
                selected.insert(super::FactId::new(format!("path/{path}"))?);
            }
        }
        Ok(selected)
    }

    /// Returns the canonical checked configuration value bound into a digest.
    #[must_use]
    pub fn config(&self) -> Datum {
        Datum::Node {
            tag: Symbol::qualified("projection", "path-selection-v1"),
            fields: vec![
                (
                    Symbol::new("include"),
                    Datum::Vector(self.includes.iter().cloned().map(Datum::String).collect()),
                ),
                (
                    Symbol::new("ignore"),
                    Datum::Vector(self.ignores.iter().cloned().map(Datum::String).collect()),
                ),
            ],
        }
    }
}

fn compile_globs(patterns: &[String]) -> Result<GlobSet, ProjectionError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        if pattern.starts_with('/') || pattern.contains('\\') {
            return Err(ProjectionError::InvalidPathSelection(format!(
                "glob is not a canonical logical path pattern: {pattern}"
            )));
        }
        let glob = GlobBuilder::new(pattern)
            .literal_separator(true)
            .backslash_escape(false)
            .build()
            .map_err(|error| ProjectionError::InvalidPathSelection(error.to_string()))?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|error| ProjectionError::InvalidPathSelection(error.to_string()))
}

fn validate_logical_path(path: &str) -> Result<(), ProjectionError> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(ProjectionError::InvalidPathSelection(format!(
            "path is not a canonical relative logical path: {path}"
        )));
    }
    Ok(())
}

/// Native baseline provider that projects all facts selected by its policy.
///
/// Each instance owns one open kind and one config Shape. Its output keeps fact
/// identity beside each semantic value so two differently named inputs cannot
/// alias even when their values are equal.
pub struct SelectFactsProvider {
    kind: ProjectionKindRef,
    config_shape: ContentId,
}

impl SelectFactsProvider {
    /// Constructs one provider instance for an open kind.
    pub fn new(kind: impl Into<String>, config_shape: ContentId) -> Result<Self, ProjectionError> {
        Ok(Self {
            kind: ProjectionKindRef::new(kind)?,
            config_shape,
        })
    }
}

impl ProjectionProvider for SelectFactsProvider {
    fn kind(&self) -> &ProjectionKindRef {
        &self.kind
    }

    fn config_shape(&self) -> &ContentId {
        &self.config_shape
    }

    fn project(
        &self,
        inputs: &ProjectionInputs,
        config: &Datum,
    ) -> Result<ProjectionOutput, ProjectionError> {
        let mut dependencies = BTreeSet::new();
        let facts = inputs
            .iter()
            .map(|(id, value)| {
                dependencies.insert(id.clone());
                Datum::Node {
                    tag: Symbol::qualified("projection", "fact-v1"),
                    fields: vec![
                        (Symbol::new("id"), Datum::String(id.as_str().to_owned())),
                        (Symbol::new("value"), value.clone()),
                    ],
                }
            })
            .collect();
        Ok(ProjectionOutput {
            value: Datum::Node {
                tag: Symbol::qualified("projection", "selected-facts-v1"),
                fields: vec![
                    (
                        Symbol::new("kind"),
                        Datum::String(self.kind.as_str().to_owned()),
                    ),
                    (Symbol::new("config"), config.clone()),
                    (Symbol::new("facts"), Datum::Vector(facts)),
                ],
            },
            dependencies,
        })
    }
}

/// Installs every baseline kind into an open registry.
pub fn install_baseline_providers(
    registry: &mut ProjectionRegistry,
    config_shape: ContentId,
    package: PackageIdentity,
) -> Result<(), ProjectionError> {
    for kind in BASELINE_PROJECTION_KINDS {
        registry.register(
            package.clone(),
            Arc::new(SelectFactsProvider::new(*kind, config_shape.clone())?),
        )?;
    }
    Ok(())
}
