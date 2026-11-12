// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Admitted registry set for the loaded promote command.
//!
//! A published row carries an exact version, a checksum, registry
//! observations, installed use, and reconciled effects. An unpublished
//! tool carries an exact source release. Dependencies precede their
//! dependents. A path source, a local receipt, a model plan, a Linux
//! requirement, and an empty set refuse. The walk does not contact a
//! registry.

use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    Registry,
    SourceRelease,
    Path,
    Receipt,
    Model,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Artifact {
    pub name: &'static str,
    pub version: &'static str,
    pub origin: Origin,
    pub identity: &'static str,
    pub installed: bool,
    pub reconciled: bool,
    pub linux: bool,
    pub ci: bool,
    pub mirror: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Need {
    pub package: &'static str,
    pub requires: &'static str,
    pub version: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Set {
    pub artifacts: &'static [Artifact],
    pub needs: &'static [Need],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Census {
    pub packages: u64,
    pub needs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fault {
    code: &'static str,
}

impl Fault {
    pub(crate) fn code(self) -> &'static str {
        self.code
    }
}

fn fault(code: &'static str) -> Fault {
    Fault { code }
}

const fn registry(name: &'static str, version: &'static str, identity: &'static str) -> Artifact {
    Artifact {
        name,
        version,
        origin: Origin::Registry,
        identity,
        installed: true,
        reconciled: true,
        linux: false,
        ci: true,
        mirror: true,
    }
}

const fn source_release(
    name: &'static str,
    version: &'static str,
    identity: &'static str,
) -> Artifact {
    Artifact {
        name,
        version,
        origin: Origin::SourceRelease,
        identity,
        installed: true,
        reconciled: true,
        linux: false,
        ci: false,
        mirror: false,
    }
}

const ARTIFACTS: &[Artifact] = &[
    registry("core", "1.0.0", "sum-core"),
    registry("boot", "1.0.1", "sum-boot"),
    source_release("tool", "0.1.0", "sum-tool"),
];

const NEEDS: &[Need] = &[
    Need {
        package: "boot",
        requires: "core",
        version: "1.0.0",
    },
    Need {
        package: "tool",
        requires: "boot",
        version: "1.0.1",
    },
];

pub(crate) const REGISTRY: Set = Set {
    artifacts: ARTIFACTS,
    needs: NEEDS,
};

const PATH_ARTIFACTS: &[Artifact] = &[
    ARTIFACTS[0],
    Artifact {
        origin: Origin::Path,
        ..ARTIFACTS[1]
    },
    ARTIFACTS[2],
];

const RECEIPT_ARTIFACTS: &[Artifact] = &[
    ARTIFACTS[0],
    Artifact {
        origin: Origin::Receipt,
        ..ARTIFACTS[1]
    },
    ARTIFACTS[2],
];

const MODEL_ARTIFACTS: &[Artifact] = &[
    ARTIFACTS[0],
    Artifact {
        origin: Origin::Model,
        ..ARTIFACTS[1]
    },
    ARTIFACTS[2],
];

const VERSION_ARTIFACTS: &[Artifact] = &[
    ARTIFACTS[0],
    Artifact {
        version: "",
        ..ARTIFACTS[1]
    },
    ARTIFACTS[2],
];

const CHECKSUM_ARTIFACTS: &[Artifact] = &[
    ARTIFACTS[0],
    Artifact {
        identity: "",
        ..ARTIFACTS[1]
    },
    ARTIFACTS[2],
];

const SOURCE_ARTIFACTS: &[Artifact] = &[
    ARTIFACTS[0],
    ARTIFACTS[1],
    Artifact {
        identity: "",
        ..ARTIFACTS[2]
    },
];

const ORDER_ARTIFACTS: &[Artifact] = &[ARTIFACTS[1], ARTIFACTS[0], ARTIFACTS[2]];

const REVISION_NEEDS: &[Need] = &[
    Need {
        package: "boot",
        requires: "core",
        version: "9.9.9",
    },
    NEEDS[1],
];

const LINUX_ARTIFACTS: &[Artifact] = &[
    ARTIFACTS[0],
    Artifact {
        linux: true,
        ..ARTIFACTS[1]
    },
    ARTIFACTS[2],
];

const INSTALL_ARTIFACTS: &[Artifact] = &[
    ARTIFACTS[0],
    Artifact {
        installed: false,
        ..ARTIFACTS[1]
    },
    ARTIFACTS[2],
];

const EFFECT_ARTIFACTS: &[Artifact] = &[
    ARTIFACTS[0],
    Artifact {
        reconciled: false,
        ..ARTIFACTS[1]
    },
    ARTIFACTS[2],
];

const OVERLAP_ARTIFACTS: &[Artifact] = &[
    ARTIFACTS[0],
    ARTIFACTS[1],
    Artifact {
        name: "boot",
        ..ARTIFACTS[2]
    },
];

const CI_ARTIFACTS: &[Artifact] = &[
    ARTIFACTS[0],
    Artifact {
        ci: false,
        ..ARTIFACTS[1]
    },
    ARTIFACTS[2],
];

const MIRROR_ARTIFACTS: &[Artifact] = &[
    ARTIFACTS[0],
    Artifact {
        mirror: false,
        ..ARTIFACTS[1]
    },
    ARTIFACTS[2],
];

const EMPTY: &[Artifact] = &[];
const NO_NEEDS: &[Need] = &[];

pub(crate) fn table(case: &str) -> Option<Set> {
    Some(match case {
        "registry" => REGISTRY,
        "path" => Set {
            artifacts: PATH_ARTIFACTS,
            needs: NEEDS,
        },
        "receipt" => Set {
            artifacts: RECEIPT_ARTIFACTS,
            needs: NEEDS,
        },
        "model" => Set {
            artifacts: MODEL_ARTIFACTS,
            needs: NEEDS,
        },
        "version" => Set {
            artifacts: VERSION_ARTIFACTS,
            needs: NEEDS,
        },
        "checksum" => Set {
            artifacts: CHECKSUM_ARTIFACTS,
            needs: NEEDS,
        },
        "source" => Set {
            artifacts: SOURCE_ARTIFACTS,
            needs: NEEDS,
        },
        "order" => Set {
            artifacts: ORDER_ARTIFACTS,
            needs: NEEDS,
        },
        "revision" => Set {
            artifacts: ARTIFACTS,
            needs: REVISION_NEEDS,
        },
        "linux" => Set {
            artifacts: LINUX_ARTIFACTS,
            needs: NEEDS,
        },
        "install" => Set {
            artifacts: INSTALL_ARTIFACTS,
            needs: NEEDS,
        },
        "unreconciled" => Set {
            artifacts: EFFECT_ARTIFACTS,
            needs: NEEDS,
        },
        "overlap" => Set {
            artifacts: OVERLAP_ARTIFACTS,
            needs: NEEDS,
        },
        "empty" => Set {
            artifacts: EMPTY,
            needs: NO_NEEDS,
        },
        "ci" => Set {
            artifacts: CI_ARTIFACTS,
            needs: NEEDS,
        },
        "mirror" => Set {
            artifacts: MIRROR_ARTIFACTS,
            needs: NEEDS,
        },
        _ => return None,
    })
}

pub(crate) fn admit(set: &Set) -> Result<Census, Fault> {
    if set.artifacts.iter().any(|artifact| artifact.linux) {
        return Err(fault("linux-requirement"));
    }
    for artifact in set.artifacts {
        if let Some(found) = origin_fault(artifact.origin) {
            return Err(found);
        }
        if artifact.version.is_empty() {
            return Err(fault("missing-version"));
        }
        if let Some(found) = identity_fault(artifact) {
            return Err(found);
        }
        if let Some(found) = observation_fault(artifact) {
            return Err(found);
        }
        if !artifact.installed {
            return Err(fault("missing-install"));
        }
        if !artifact.reconciled {
            return Err(fault("unreconciled"));
        }
    }
    if overlapped(set) {
        return Err(fault("overlap"));
    }
    if set.artifacts.is_empty() {
        return Err(fault("missing-package"));
    }
    if unordered(set) {
        return Err(fault("dependency-order"));
    }
    Ok(Census {
        packages: u64::try_from(set.artifacts.len()).unwrap_or(u64::MAX),
        needs: u64::try_from(set.needs.len()).unwrap_or(u64::MAX),
    })
}

fn origin_fault(origin: Origin) -> Option<Fault> {
    match origin {
        Origin::Path => Some(fault("path-source")),
        Origin::Receipt => Some(fault("local-receipt")),
        Origin::Model => Some(fault("model-plan")),
        Origin::Registry | Origin::SourceRelease => None,
    }
}

fn identity_fault(artifact: &Artifact) -> Option<Fault> {
    if !artifact.identity.is_empty() {
        return None;
    }
    Some(match artifact.origin {
        Origin::SourceRelease => fault("missing-source"),
        Origin::Registry | Origin::Path | Origin::Receipt | Origin::Model => {
            fault("missing-checksum")
        }
    })
}

fn observation_fault(artifact: &Artifact) -> Option<Fault> {
    if artifact.origin != Origin::Registry {
        return None;
    }
    if !artifact.ci {
        return Some(fault("missing-ci"));
    }
    if !artifact.mirror {
        return Some(fault("missing-mirror"));
    }
    None
}

fn overlapped(set: &Set) -> bool {
    let mut names = BTreeSet::new();
    set.artifacts
        .iter()
        .any(|artifact| !names.insert(artifact.name))
}

fn unordered(set: &Set) -> bool {
    let mut index = BTreeMap::new();
    for (at, artifact) in set.artifacts.iter().enumerate() {
        index.insert(artifact.name, at);
    }
    set.needs.iter().any(|need| {
        let Some(&package_at) = index.get(need.package) else {
            return true;
        };
        let Some(&required_at) = index.get(need.requires) else {
            return true;
        };
        required_at >= package_at || set.artifacts[required_at].version != need.version
    })
}

#[cfg(test)]
mod tests {
    use super::{ARTIFACTS, Artifact, NO_NEEDS, Origin, Set, admit};

    const LINUX_PATH: &[Artifact] = &[Artifact {
        origin: Origin::Path,
        linux: true,
        ..ARTIFACTS[1]
    }];

    #[test]
    fn a_linux_requirement_wins_over_a_path_source() {
        let set = Set {
            artifacts: LINUX_PATH,
            needs: NO_NEEDS,
        };
        let fault = admit(&set).expect_err("refused");
        assert_eq!(fault.code(), "linux-requirement");
    }
}
