//! Frozen fidelity statement for the embedded Python profile.

/// Independently reported fidelity dimensions; never a blanket compatibility claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PythonFidelity {
    /// Frozen syntax authority accepted by `codec/python`.
    pub syntax: &'static str,
    /// Stable representation produced by the codec.
    pub lowering: &'static str,
    /// Behavior executed without a compiler or foreign VM.
    pub direct_evaluation: &'static str,
    /// Object and control behavior composed from shared organs.
    pub object_control: &'static str,
    /// Library and import boundary.
    pub module_library: &'static str,
    /// Resource and authority boundary.
    pub boundedness: &'static str,
    /// Expected, deliberately unclaimed surface.
    pub expected_gaps: &'static [&'static str],
}

/// One frozen differential or regression case.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PythonEvidenceCase {
    /// Stable case name.
    pub name: &'static str,
    /// Behavior dimension exercised by the case.
    pub dimension: &'static str,
    /// Python source supplied to the SIM codec.
    pub source: &'static str,
    /// Frozen observation or fail-closed outcome.
    pub expected: &'static str,
    /// Whether the expected value came from the frozen external oracle.
    pub external_oracle: bool,
}

/// Exact external oracle used to freeze simple differential expectations.
/// It is evidence only: never linked, spawned, imported, or used at runtime.
pub const PYTHON_EXTERNAL_ORACLE: &str = "CPython 3.14.6 (offline expected-value oracle only)";

/// Honest public fidelity statement for this profile.
pub const PYTHON_FIDELITY: PythonFidelity = PythonFidelity {
    syntax: "bounded lossless Python 3.14.6 concrete syntax; admission is not execution support",
    lowering: "stable python/module, python/statement, and python/token expressions",
    direct_evaluation: "bounded tree-walking scalar expressions and assignments over lowered forms",
    object_control: "checked classes/descriptors, exceptions, generators, matching, and managed cycles through shared organs",
    module_library: "matrix-listed core plus caller-supplied Dir imports; no pip, host path, or ambient standard library",
    boundedness: "codec budgets, evaluator step limits, managed-heap limits, and explicit capabilities",
    expected_gaps: &[
        "not a CPython replacement",
        "no bytecode, compiler IR, optimizer, or foreign VM",
        "no asyncio event loop, pip, ambient IO, or host import search",
        "syntax coverage does not imply direct-evaluation coverage",
    ],
};

/// Frozen cases spanning the profile's claimed and refused behavior.
pub const PYTHON_EVIDENCE_CASES: &[PythonEvidenceCase] = &[
    PythonEvidenceCase {
        name: "call",
        dimension: "direct-evaluation",
        source: "f = lambda x: x + 2\nf(40)\n",
        expected: "42",
        external_oracle: true,
    },
    PythonEvidenceCase {
        name: "cyclic-value",
        dimension: "object/control",
        source: "value = []\nvalue.append(value)\nvalue is value[0]\n",
        expected: "true; shared bounded heap",
        external_oracle: true,
    },
    PythonEvidenceCase {
        name: "exception",
        dimension: "object/control",
        source: "try:\n raise ValueError('x')\nexcept ValueError:\n answer = 42\n",
        expected: "42",
        external_oracle: true,
    },
    PythonEvidenceCase {
        name: "generator",
        dimension: "object/control",
        source: "def g():\n yield 40\n yield 2\nsum(g())\n",
        expected: "42",
        external_oracle: true,
    },
    PythonEvidenceCase {
        name: "matching",
        dimension: "object/control",
        source: "match [40, 2]:\n case [a, b]: answer = a + b\n",
        expected: "42",
        external_oracle: true,
    },
    PythonEvidenceCase {
        name: "supplied-import",
        dimension: "module/library",
        source: "from supplied import answer\nanswer\n",
        expected: "42 from caller-supplied Dir only",
        external_oracle: false,
    },
    PythonEvidenceCase {
        name: "ambient-import-refusal",
        dimension: "module/library",
        source: "import os\n",
        expected: "refused without a supplied module root",
        external_oracle: false,
    },
    PythonEvidenceCase {
        name: "eval-capability-refusal",
        dimension: "boundedness",
        source: "eval('40 + 2')\n",
        expected: "refused without read-eval and diminished capabilities",
        external_oracle: false,
    },
    PythonEvidenceCase {
        name: "exec-capability-refusal",
        dimension: "boundedness",
        source: "exec('answer = 42')\n",
        expected: "refused without read-eval and diminished capabilities",
        external_oracle: false,
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeSet, fs, path::Path};

    #[test]
    fn evidence_covers_every_required_regression_family() {
        let names = PYTHON_EVIDENCE_CASES
            .iter()
            .map(|case| case.name)
            .collect::<BTreeSet<_>>();
        for required in [
            "call",
            "cyclic-value",
            "exception",
            "generator",
            "matching",
            "supplied-import",
            "ambient-import-refusal",
            "eval-capability-refusal",
            "exec-capability-refusal",
        ] {
            assert!(
                names.contains(required),
                "missing Python evidence case {required}"
            );
        }
        assert!(
            PYTHON_EVIDENCE_CASES
                .iter()
                .any(|case| case.external_oracle)
        );
        assert!(
            PYTHON_EVIDENCE_CASES
                .iter()
                .any(|case| !case.external_oracle)
        );
    }

    #[test]
    fn crate_has_no_foreign_python_or_compiler_dependency_or_artifact() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let manifest = fs::read_to_string(root.join("Cargo.toml"))
            .unwrap()
            .to_ascii_lowercase();
        for forbidden in ["pyo3", "cpython =", "python3-sys", "python27-sys"] {
            assert!(
                !manifest.contains(forbidden),
                "foreign Python dependency {forbidden}"
            );
        }
        fn scan(path: &Path) {
            for entry in fs::read_dir(path).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    scan(&path);
                    continue;
                }
                let extension = path
                    .extension()
                    .and_then(|value| value.to_str())
                    .unwrap_or("");
                assert!(
                    !matches!(extension, "pyc" | "pyo" | "pickle"),
                    "foreign Python artifact {}",
                    path.display()
                );
            }
        }
        scan(root);
        let runtime = fs::read_to_string(root.join("src/runtime.rs")).unwrap();
        for forbidden in [
            "Py_CompileString",
            "PyEval_",
            "Command::new(\"python\"",
            "Command::new(\"python3\"",
        ] {
            assert!(
                !runtime.contains(forbidden),
                "compiler/VM fallback marker {forbidden}"
            );
        }
    }
}
