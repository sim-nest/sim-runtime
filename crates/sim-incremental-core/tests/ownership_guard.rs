// conformance: the incremental core retains the generic dataflow worklist boundary.

//! Structural ownership guard for the generic dataflow worklist.

use std::{fs, path::PathBuf};

const SHADOW_ENGINE: &str = r#"
pub struct Solver<N, S> {
    frontier: std::collections::VecDeque<N>,
    memo: std::collections::BTreeMap<N, S>,
}
"#;

const TRANSFER_RULES_ONLY: &str = r#"
pub trait Transfer<S> { fn transfer(&self, input: &S) -> S; }
pub struct DefiniteAssignment { generated: std::collections::BTreeSet<u32> }
"#;

#[derive(Debug)]
struct Policy {
    owner: String,
    frontier_types: Vec<String>,
    retained_state_types: Vec<String>,
    classified_crates: Vec<(String, String, String)>,
}

impl Policy {
    fn load(root: &std::path::Path) -> Self {
        let source = fs::read_to_string(root.join("dataflow-ownership.toml"))
            .expect("dataflow ownership policy must exist");
        assert_eq!(scalar(&source, "schema"), "sim.dataflow-ownership/v1");
        let mut classified_crates = Vec::new();
        let mut has_transfer_classifier = false;
        for row in source.split("[[classifier]]").skip(1) {
            let class = scalar(row, "class");
            let reason = scalar(row, "reason");
            assert!(!reason.is_empty(), "every classifier requires a reason");
            has_transfer_classifier |= class == "domain-transfer-rules";
            for name in array(row, "crates") {
                classified_crates.push((name, class.clone(), reason.clone()));
            }
        }
        assert!(has_transfer_classifier);
        Self {
            owner: scalar(&source, "owner"),
            frontier_types: array(&source, "frontier_types"),
            retained_state_types: array(&source, "retained_state_types"),
            classified_crates,
        }
    }

    fn findings(&self, source: &str) -> Vec<String> {
        public_structs(source)
            .into_iter()
            .filter(|item| self.frontier_types.iter().any(|kind| item.contains(kind)))
            .filter(|item| {
                self.retained_state_types
                    .iter()
                    .any(|kind| item.contains(kind))
            })
            .map(|item| format!("generic memoized worklist: {}", first_line(&item)))
            .collect()
    }

    fn classifier(&self, crate_name: &str) -> Option<(&str, &str)> {
        self.classified_crates
            .iter()
            .find(|(name, _, _)| name == crate_name)
            .map(|(_, class, reason)| (class.as_str(), reason.as_str()))
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
    let Some(start) = source.find(&format!("{key} = [")) else {
        return Vec::new();
    };
    let rest = &source[start..];
    let body = rest
        .split_once('[')
        .and_then(|(_, rest)| rest.split_once(']'))
        .map(|(body, _)| body)
        .unwrap_or_default();
    body.split(',')
        .filter_map(|item| item.trim().strip_prefix('"')?.strip_suffix('"'))
        .map(str::to_owned)
        .collect()
}

fn public_structs(source: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = None::<(String, i32)>;
    for line in source.lines() {
        if current.is_none() && line.trim_start().starts_with("pub struct ") {
            let depth = brace_delta(line);
            if depth == 0 && line.contains(';') {
                continue;
            }
            current = Some((format!("{line}\n"), depth));
            continue;
        }
        if let Some((text, depth)) = &mut current {
            text.push_str(line);
            text.push('\n');
            *depth += brace_delta(line);
            if *depth <= 0 {
                items.push(std::mem::take(text));
                current = None;
            }
        }
    }
    items
}

fn brace_delta(line: &str) -> i32 {
    line.bytes()
        .map(|byte| match byte {
            b'{' => 1,
            b'}' => -1,
            _ => 0,
        })
        .sum()
}

fn first_line(item: &str) -> &str {
    item.lines().next().unwrap_or("public struct")
}

fn repository_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .canonicalize()
        .expect("incremental source directory must resolve");
    loop {
        if path.join("dataflow-ownership.toml").is_file() {
            return path;
        }
        assert!(path.pop(), "dataflow ownership policy repository not found");
    }
}

#[test]
fn guard_rejects_a_shadow_engine_and_admits_transfer_rules() {
    let root = repository_root();
    let policy = Policy::load(&root);
    assert_eq!(policy.findings(SHADOW_ENGINE).len(), 1);
    assert!(policy.findings(TRANSFER_RULES_ONLY).is_empty());
}

#[test]
fn runtime_crates_have_no_unclassified_shadow_engine() {
    let root = repository_root();
    let policy = Policy::load(&root);
    for entry in fs::read_dir(root.join("crates")).unwrap() {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }
        let crate_name = entry.file_name().to_string_lossy().into_owned();
        if crate_name == policy.owner {
            continue;
        }
        if let Some((class, reason)) = policy.classifier(&crate_name) {
            assert!(!class.is_empty() && !reason.is_empty());
            continue;
        }
        let mut paths = vec![entry.path().join("src")];
        while let Some(path) = paths.pop() {
            if !path.exists() {
                continue;
            }
            for child in fs::read_dir(path).unwrap() {
                let child = child.unwrap();
                if child.file_type().unwrap().is_dir() {
                    paths.push(child.path());
                } else if child.path().extension().is_some_and(|ext| ext == "rs") {
                    let source = fs::read_to_string(child.path()).unwrap();
                    let findings = policy.findings(&source);
                    assert!(
                        findings.is_empty(),
                        "{} declares a second dataflow engine without an explicit classifier reason: {findings:?}",
                        child.path().display()
                    );
                }
            }
        }
    }
}
