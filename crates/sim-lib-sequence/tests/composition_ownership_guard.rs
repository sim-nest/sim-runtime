//! Structural guard for guest JSON and collection composition.

// conformance: guest collections reuse the shared ordered sequence substrate.

use std::{
    fs,
    path::{Path, PathBuf},
};

const PRIVATE_ORDERED_TABLE: &str = r#"
struct GuestOrderedTable<K, V> {
    rows: Vec<(K, V)>,
    key_index: std::collections::BTreeMap<K, usize>,
}
"#;

const POLICY_WRAPPER: &str = r#"
use sim_lib_sequence::{OrderedTable, SparseSequence};
struct JavascriptArray<V> { elements: SparseSequence<V> }
struct JavascriptMap<K, V> { entries: OrderedTable<K, V, SameValueZero> }
"#;

#[derive(Debug)]
struct Relationship {
    path: String,
    class: String,
    owner: String,
    reason: String,
}

#[derive(Debug)]
struct Policy {
    json_owner: String,
    sequence_owner: String,
    remediation: String,
    sparse_storage_fields: Vec<String>,
    sparse_length_fields: Vec<String>,
    ordered_storage_fields: Vec<String>,
    ordered_index_fields: Vec<String>,
    json_driver_names: Vec<String>,
    relationships: Vec<Relationship>,
}

impl Policy {
    fn load(root: &Path) -> Self {
        let source = fs::read_to_string(root.join("composition-ownership.toml"))
            .expect("composition ownership policy must exist");
        assert_eq!(scalar(&source, "schema"), "sim.composition-ownership/v1");
        Self {
            json_owner: scalar(&source, "json_owner"),
            sequence_owner: scalar(&source, "sequence_owner"),
            remediation: scalar(&source, "remediation"),
            sparse_storage_fields: array(&source, "sparse_storage_fields"),
            sparse_length_fields: array(&source, "sparse_length_fields"),
            ordered_storage_fields: array(&source, "ordered_storage_fields"),
            ordered_index_fields: array(&source, "ordered_index_fields"),
            json_driver_names: array(&source, "json_driver_names"),
            relationships: sections(&source, "[[approved_relationship]]")
                .map(|row| Relationship {
                    path: scalar(row, "path"),
                    class: scalar(row, "class"),
                    owner: scalar(row, "owner"),
                    reason: scalar(row, "reason"),
                })
                .collect(),
        }
    }

    fn findings(&self, path: &Path, source: &str) -> Vec<String> {
        let relative = path.to_string_lossy().replace('\\', "/");
        if let Some(row) = self
            .relationships
            .iter()
            .find(|row| relative.ends_with(&row.path))
        {
            assert_eq!(row.class, "policy-wrapper");
            assert!(row.owner == self.json_owner || row.owner == self.sequence_owner);
            assert!(!row.reason.is_empty());
        }

        let mut findings = Vec::new();
        for item in structs(source) {
            let fields = field_names(&item);
            let sparse = field_pair(
                &fields,
                &self.sparse_storage_fields,
                &self.sparse_length_fields,
            );
            let ordered = field_pair(
                &fields,
                &self.ordered_storage_fields,
                &self.ordered_index_fields,
            );
            if sparse.is_some() && (item.contains("Vec<Option<") || item.contains("BTreeMap<usize"))
            {
                findings.push(format!(
                    "{relative} retains a private sparse store; owner: {}; remediation: {}",
                    self.sequence_owner, self.remediation
                ));
            }
            if ordered.is_some() && item.contains("Vec<(") {
                findings.push(format!(
                    "{relative} retains a private ordered table; owner: {}; remediation: {}",
                    self.sequence_owner, self.remediation
                ));
            }
        }
        for name in &self.json_driver_names {
            if source.contains(&format!("fn {name}(")) {
                findings.push(format!(
                    "{relative} declares private JSON parsing or rendering `{name}`; owner: {}; remediation: {}",
                    self.json_owner, self.remediation
                ));
            }
        }
        findings.sort();
        findings.dedup();
        findings
    }
}

fn field_pair<'a>(
    fields: &'a [String],
    left: &[String],
    right: &[String],
) -> Option<(&'a str, &'a str)> {
    Some((
        fields.iter().find(|field| left.contains(field))?,
        fields.iter().find(|field| right.contains(field))?,
    ))
}

fn sections<'a>(source: &'a str, heading: &str) -> impl Iterator<Item = &'a str> {
    source
        .split(heading)
        .skip(1)
        .map(|row| row.split("[[").next().unwrap_or(row))
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
    let Some(line) = source
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with(&format!("{key} = [")))
    else {
        return Vec::new();
    };
    line.split_once('[')
        .and_then(|(_, rest)| rest.rsplit_once(']'))
        .map(|(body, _)| body)
        .unwrap_or_default()
        .split(',')
        .filter_map(|item| item.trim().strip_prefix('"')?.strip_suffix('"'))
        .map(str::to_owned)
        .collect()
}

fn structs(source: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = None::<(String, i32)>;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if current.is_none()
            && (trimmed.starts_with("struct ") || trimmed.starts_with("pub struct "))
        {
            current = Some((format!("{line}\n"), brace_delta(line)));
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

fn brace_delta(line: &str) -> i32 {
    line.bytes()
        .map(|byte| match byte {
            b'{' => 1,
            b'}' => -1,
            _ => 0,
        })
        .sum()
}

fn repository_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    while !path.join("composition-ownership.toml").is_file() {
        assert!(path.pop(), "composition ownership repository not found");
    }
    path
}

fn rust_sources(path: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![path.to_owned()];
    while let Some(path) = pending.pop() {
        for entry in fs::read_dir(path).expect("source directory") {
            let entry = entry.expect("source entry");
            if entry.file_type().expect("source type").is_dir() {
                pending.push(entry.path());
            } else if entry.path().extension().is_some_and(|ext| ext == "rs") {
                files.push(entry.path());
            }
        }
    }
    files
}

#[test]
fn guard_rejects_private_ordered_table_and_admits_javascript_policy_wrapper() {
    let policy = Policy::load(&repository_root());
    let findings = policy.findings(
        Path::new("crates/sim-lib-lang-example/src/collections.rs"),
        PRIVATE_ORDERED_TABLE,
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("private ordered table"))
    );
    assert!(
        policy
            .findings(
                Path::new("crates/sim-lib-lang-javascript/src/collections.rs"),
                POLICY_WRAPPER,
            )
            .is_empty()
    );
}

#[test]
fn guest_sources_compose_json_and_sequence_owners_without_private_mechanics() {
    let root = repository_root();
    let policy = Policy::load(&root);
    let mut findings = Vec::new();
    for path in rust_sources(&root.join("crates")) {
        let relative = path.strip_prefix(&root).expect("repository source");
        if relative.to_string_lossy().contains("sim-lib-lang-") {
            let source = fs::read_to_string(&path).expect("guest source");
            findings.extend(policy.findings(relative, &source));
        }
    }
    assert!(findings.is_empty(), "{}", findings.join("\n"));
}
