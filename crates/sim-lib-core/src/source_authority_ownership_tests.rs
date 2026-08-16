// conformance: source authority remains owned by the shared runtime core.

//! Structural source-fact guard for the shared source-authority boundary.

use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug)]
struct Policy {
    owner: String,
    remediation: String,
    guest_prefix: String,
    forbidden_suffixes: Vec<String>,
    authority_fields: Vec<String>,
    guests: Vec<(String, String)>,
}

impl Policy {
    fn load(root: &Path) -> Self {
        let source = fs::read_to_string(root.join("source-authority.toml")).unwrap();
        assert_eq!(
            scalar(&source, "schema"),
            "sim.source-authority-ownership/v1"
        );
        Self {
            owner: scalar(&source, "owner"),
            remediation: scalar(&source, "remediation"),
            guest_prefix: scalar(&source, "guest_crate_prefix"),
            forbidden_suffixes: array(&source, "forbidden_type_suffixes"),
            authority_fields: array(&source, "authority_fields"),
            guests: source
                .split("[[guest]]")
                .skip(1)
                .map(|row| (scalar(row, "crate"), scalar(row, "status")))
                .collect(),
        }
    }

    fn findings(&self, relative: &Path, source: &str) -> Vec<String> {
        let path = relative.to_string_lossy();
        if !path
            .split('/')
            .any(|part| part.starts_with(&self.guest_prefix))
        {
            return Vec::new();
        }
        let mut findings = Vec::new();
        for item in structs(source) {
            let name = struct_name(&item);
            let fields = field_names(&item);
            let authority_count = fields
                .iter()
                .filter(|field| self.authority_fields.contains(field))
                .count();
            if authority_count > 1 {
                findings.push(format!("{path} repeats {authority_count} source-authority fields in {name}; owner: {}; remediation: {}", self.owner, self.remediation));
            }
            if self
                .forbidden_suffixes
                .iter()
                .any(|suffix| name.ends_with(suffix))
            {
                findings.push(format!("{path} declares generic guest admission type {name}; owner: {}; remediation: {}", self.owner, self.remediation));
            }
        }
        findings
    }
}

fn scalar(source: &str, key: &str) -> String {
    source
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(&format!("{key} = \"")))
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_default()
        .to_owned()
}

fn array(source: &str, key: &str) -> Vec<String> {
    source
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(&format!("{key} = [")))
        .and_then(|line| line.split_once('['))
        .and_then(|(_, rest)| rest.rsplit_once(']'))
        .map(|(body, _)| {
            body.split(',')
                .filter_map(|item| item.trim().strip_prefix('"')?.strip_suffix('"'))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn structs(source: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = None::<(String, i32)>;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if current.is_none()
            && (trimmed.starts_with("struct ") || trimmed.starts_with("pub struct "))
        {
            current = Some((format!("{line}\n"), braces(line)));
        } else if let Some((text, depth)) = &mut current {
            text.push_str(line);
            text.push('\n');
            *depth += braces(line);
            if *depth <= 0 {
                result.push(std::mem::take(text));
                current = None;
            }
        }
    }
    result
}

fn braces(line: &str) -> i32 {
    line.bytes()
        .map(|b| match b {
            b'{' => 1,
            b'}' => -1,
            _ => 0,
        })
        .sum()
}
fn struct_name(item: &str) -> String {
    item.split_whitespace()
        .skip_while(|word| *word != "struct")
        .nth(1)
        .unwrap_or("")
        .split(['<', '{'])
        .next()
        .unwrap_or("")
        .to_owned()
}
fn field_names(item: &str) -> Vec<String> {
    item.lines()
        .skip(1)
        .filter_map(|line| {
            line.trim()
                .trim_start_matches("pub ")
                .split_once(':')
                .map(|(name, _)| name.trim())
        })
        .filter(|name| {
            name.chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        })
        .map(str::to_owned)
        .collect()
}
fn root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut path = fs::canonicalize(manifest.join("src"))
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .unwrap_or(manifest);
    while !path.join("source-authority.toml").is_file() {
        assert!(path.pop());
    }
    path
}
fn rust_sources(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![path.to_owned()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                pending.push(entry.path());
            } else if entry.path().extension().is_some_and(|ext| ext == "rs") {
                files.push(entry.path());
            }
        }
    }
    files
}

#[test]
fn fixture_allows_semantic_wrapper_but_rejects_shadow_authority() {
    let policy = Policy::load(&root());
    assert!(
        policy
            .findings(
                Path::new("crates/sim-lib-lang-example/src/eval.rs"),
                "struct GuestEvalSemantics {\n    mode: EvalMode,\n}"
            )
            .is_empty()
    );
    let findings = policy.findings(
        Path::new("crates/sim-lib-lang-example/src/eval.rs"),
        "struct DynamicAdmission {\n    read_policy: ReadPolicy,\n    requires: Vec<CapabilityName>,\n    allow: CapabilitySet,\n}",
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("repeats 3 source-authority fields"))
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("generic guest admission type"))
    );
}

#[test]
fn every_registered_guest_is_classified_and_sources_have_no_shadow_envelope() {
    let root = root();
    let policy = Policy::load(&root);
    let profiles = fs::read_to_string(root.join("guest-profiles.toml")).unwrap();
    let registered = profiles
        .split("[[guest]]")
        .skip(1)
        .map(|row| scalar(row, "crate"))
        .collect::<Vec<_>>();
    assert_eq!(registered.len(), policy.guests.len());
    for guest in registered {
        assert!(
            policy
                .guests
                .iter()
                .any(|(name, status)| name == &guest
                    && matches!(status.as_str(), "wired" | "unwired")),
            "unclassified guest {guest}"
        );
    }
    let mut findings = Vec::new();
    for path in rust_sources(&root.join("crates")) {
        let relative = path.strip_prefix(&root).unwrap();
        findings.extend(policy.findings(relative, &fs::read_to_string(&path).unwrap()));
    }
    assert!(findings.is_empty(), "{}", findings.join("\n"));
}
