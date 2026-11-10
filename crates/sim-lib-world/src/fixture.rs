// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Caller-named batch data for snapshot, proof, and source admission.
//!
//! A batch name selects one static document. The same document may be
//! published under more than one name. Interpretation uses the document's
//! rows, proofs, and resolver text. It does not inspect the name. An
//! undeclared provider returns a gap. A model remote stays unavailable.

/// One explicit observation row in a caller-named batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    /// Domain port that owns the selector.
    pub(crate) port: &'static str,
    /// Exact selector the port can answer.
    pub(crate) selector: &'static str,
    /// Observation status. A value is present only for `observed`.
    pub(crate) status: &'static str,
    /// Semantic value bound to an observed row.
    pub(crate) value: Option<&'static str>,
}

/// One declared proof consumed by a caller-named conclusion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Proof {
    /// Open projection kind already loaded for this fact.
    pub(crate) kind: &'static str,
    /// Fact identity the conclusion declares.
    pub(crate) fact: &'static str,
    /// Owner that declares the conclusion.
    pub(crate) owner: &'static str,
    /// Conclusion that consumes the fact.
    pub(crate) conclusion: &'static str,
}

const WIDGET: &[Row] = &[
    Row {
        port: "provider",
        selector: "provider/widget",
        status: "observed",
        value: Some("widget-body"),
    },
    Row {
        port: "provider",
        selector: "provider/generated",
        status: "observed",
        value: Some("generated-widget"),
    },
    Row {
        port: "remote",
        selector: "remote/hostile",
        status: "unavailable",
        value: None,
    },
];

const WIDGET_REVISED: &[Row] = &[
    Row {
        port: "provider",
        selector: "provider/widget",
        status: "observed",
        value: Some("widget-next"),
    },
    Row {
        port: "provider",
        selector: "provider/generated",
        status: "observed",
        value: Some("generated-widget"),
    },
    Row {
        port: "remote",
        selector: "remote/hostile",
        status: "unavailable",
        value: None,
    },
];

const PROOFS: &[Proof] = &[
    Proof {
        kind: "world/package-assembly-v1",
        fact: "fact/lantern-source",
        owner: "lantern/widget",
        conclusion: "conclusion/lantern-proof",
    },
    Proof {
        kind: "world/generated-ownership-v1",
        fact: "fact/lantern-generated",
        owner: "lantern/widget",
        conclusion: "conclusion/lantern-proof",
    },
];

struct NamedRows {
    name: &'static str,
    rows: &'static [Row],
}

const OBSERVATIONS: &[NamedRows] = &[
    NamedRows {
        name: "lantern",
        rows: WIDGET,
    },
    NamedRows {
        name: "lantern-alias",
        rows: WIDGET,
    },
    NamedRows {
        name: "lantern-revised",
        rows: WIDGET_REVISED,
    },
];

struct NamedText {
    name: &'static str,
    text: &'static str,
}

const SEALS: &[NamedText] = &[
    NamedText {
        name: "lantern",
        text: LANTERN,
    },
    NamedText {
        name: "lantern-alias",
        text: LANTERN,
    },
    NamedText {
        name: "lantern-revised",
        text: LANTERN_REVISED,
    },
    NamedText {
        name: "lantern-stub",
        text: LANTERN_STUB,
    },
];

const LANTERN: &str = "\
sim-world-inputs-v1
config lock 4
config resolver 3
root-package lantern
sealed-target x86_64-unknown-linux-gnu
toolchain commit aaa
toolchain host x86_64-unknown-linux-gnu
toolchain release 1.0.0
feature lantern native-export [\"dep:lantern-codec\"]
domain resolver/lantern . compiling Cargo.lock
domain resolver/lantern/vendor/widgets vendor/widgets enumerated vendor/widgets/Cargo.lock
alias resolver/lantern .
member resolver/lantern path lantern 1.0.0 crates/lantern abcdef path-content closure
member resolver/lantern/vendor/widgets path widget 0.1.0 vendor/widgets/widget 123456 path-content closure
deps resolver/lantern lantern 1.0.0 crates/lantern widget~0.1.0
deps resolver/lantern/vendor/widgets widget 0.1.0 vendor/widgets/widget
generated resolver/lantern src/generated.rs 16 aaaa
path-files resolver/lantern crates/lantern 2
path-files resolver/lantern/vendor/widgets vendor/widgets/widget 1
";

const LANTERN_REVISED: &str = "\
sim-world-inputs-v1
config lock 4
config resolver 3
root-package lantern
sealed-target x86_64-unknown-linux-gnu
toolchain commit aaa
toolchain host x86_64-unknown-linux-gnu
toolchain release 1.0.0
feature lantern native-export [\"dep:lantern-codec\"]
domain resolver/lantern . compiling Cargo.lock
domain resolver/lantern/vendor/widgets vendor/widgets enumerated vendor/widgets/Cargo.lock
alias resolver/lantern .
member resolver/lantern path lantern 1.0.0 crates/lantern abcdef path-content closure
member resolver/lantern/vendor/widgets path widget 0.1.0 vendor/widgets/widget 123456 path-content closure
deps resolver/lantern lantern 1.0.0 crates/lantern widget~0.1.0
deps resolver/lantern/vendor/widgets widget 0.1.0 vendor/widgets/widget
generated resolver/lantern src/generated.rs 16 bbbb
path-files resolver/lantern crates/lantern 2
path-files resolver/lantern/vendor/widgets vendor/widgets/widget 1
";

const LANTERN_STUB: &str = "\
sim-world-inputs-v1
config lock 4
config resolver 3
root-package lantern
sealed-target x86_64-unknown-linux-gnu
toolchain commit aaa
toolchain host x86_64-unknown-linux-gnu
toolchain release 1.0.0
feature lantern native-export [\"dep:lantern-codec\"]
domain resolver/lantern . compiling Cargo.lock
domain resolver/lantern/vendor/widgets vendor/widgets enumerated vendor/widgets/Cargo.lock
alias resolver/lantern .
member resolver/lantern stub lantern 1.0.0 crates/lantern - metadata closure
member resolver/lantern/vendor/widgets path widget 0.1.0 vendor/widgets/widget 123456 path-content closure
deps resolver/lantern lantern 1.0.0 crates/lantern widget~0.1.0
deps resolver/lantern/vendor/widgets widget 0.1.0 vendor/widgets/widget
generated resolver/lantern src/generated.rs 16 aaaa
path-files resolver/lantern/vendor/widgets vendor/widgets/widget 1
";

/// Reports whether `name` is a construction fixture batch.
pub(crate) fn is_construction_name(name: &str) -> bool {
    rows(name).is_some() || seal_text(name).is_some()
}

/// Returns every construction observation row with its batch name.
pub(crate) fn observation_rows() -> impl Iterator<Item = (&'static str, Row)> {
    OBSERVATIONS
        .iter()
        .flat_map(|named| named.rows.iter().copied().map(move |row| (named.name, row)))
}

/// Returns the observation rows published under `batch`.
pub(crate) fn rows(batch: &str) -> Option<&'static [Row]> {
    let found = OBSERVATIONS
        .iter()
        .find(|item| item.name == batch)
        .map(|item| item.rows);
    if let Some(rows) = found {
        debug_assert!(rows.iter().all(|row| {
            row.selector
                .strip_prefix(row.port)
                .is_some_and(|rest| rest.starts_with('/'))
        }));
    }
    found
}

/// Returns the resolver text published under `batch`.
pub(crate) fn seal_text(batch: &str) -> Option<&'static str> {
    SEALS
        .iter()
        .find(|item| item.name == batch)
        .map(|item| item.text)
}

/// Returns the proof declarations shared by every caller-named batch.
pub(crate) const fn proofs() -> &'static [Proof] {
    PROOFS
}
