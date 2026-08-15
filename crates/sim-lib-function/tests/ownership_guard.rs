//! Source-fact ownership guard for neutral function declarations and captures.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

const SHADOW_FUNCTION: &str = r#"
pub struct GuestFunction {
    parameter_names: Vec<Symbol>,
    capture_slots: Vec<Capture>,
    body_policy: Policy,
}
"#;

const PRIVATE_CAPTURE_GRAPH: &str = r#"
struct PrivateEnvironment {
    bindings: Vec<BindingCell>,
    outer: Option<Box<PrivateEnvironment>>,
}
"#;

const LANGUAGE_POLICY_ONLY: &str = r#"
pub struct GuestFrame {
    coroutine_flags: Flags,
    traceback_state: Traceback,
    body_policy: Policy,
}
"#;

const DICTIONARY_FUNCTION_ADAPTER: &str = r#"
pub fn adapt_typeclass_dictionary(dictionary: TypeclassDictionary) -> FunctionInstance<DictionaryBody> {
    FunctionInstance::new(dictionary)
}
"#;

const DICTIONARY_MANAGED_EDGES: &str = r#"
pub struct TypeclassDictionary {
    managed_edges: Vec<ManagedHandle>,
}
"#;

#[derive(Debug)]
struct Policy {
    owner: String,
    remediation: String,
    guest_crate_prefix: String,
    parameter_fields: Vec<String>,
    capture_fields: Vec<String>,
    private_graph_storage_fields: Vec<String>,
    private_graph_edge_fields: Vec<String>,
    approved_language_policy: Vec<(String, String)>,
    non_participants: Vec<NonParticipant>,
}

#[derive(Debug)]
struct NonParticipant {
    model: String,
    terms: Vec<String>,
    mismatch_code: String,
    reason: String,
    approved_relationships: Vec<String>,
}

impl Policy {
    fn load(root: &Path) -> Self {
        let source = fs::read_to_string(root.join("function-ownership.toml"))
            .expect("function ownership policy must exist");
        assert_eq!(scalar(&source, "schema"), "sim.function-ownership/v1");
        let owner = scalar(&source, "owner");
        let remediation = scalar(&source, "remediation");
        assert!(!owner.is_empty() && !remediation.is_empty());
        let non_participants = sections(&source, "[[non_participant]]")
            .map(|row| NonParticipant {
                model: scalar(row, "model"),
                terms: array(row, "source_terms"),
                mismatch_code: scalar(row, "mismatch_code"),
                reason: scalar(row, "reason"),
                approved_relationships: array(row, "approved_relationships"),
            })
            .collect::<Vec<_>>();
        assert!(!non_participants.is_empty());
        Self {
            owner,
            remediation,
            guest_crate_prefix: scalar(&source, "guest_crate_prefix"),
            parameter_fields: array(&source, "parameter_fields"),
            capture_fields: array(&source, "capture_fields"),
            private_graph_storage_fields: array(&source, "private_graph_storage_fields"),
            private_graph_edge_fields: array(&source, "private_graph_edge_fields"),
            approved_language_policy: sections(&source, "[[approved_language_policy]]")
                .map(|row| (scalar(row, "path"), scalar(row, "reason")))
                .collect(),
            non_participants,
        }
    }

    fn findings(&self, path: &Path, source: &str) -> Vec<String> {
        let mut findings = public_structs(source)
            .into_iter()
            .filter_map(|item| {
                let fields = field_names(&item);
                let parameter = fields.iter().find(|field| self.parameter_fields.contains(field))?;
                let capture = fields.iter().find(|field| self.capture_fields.contains(field))?;
                Some(format!(
                    "{} combines neutral `{parameter}` and `{capture}` fields; owner: {}; remediation: {}",
                    path.display(), self.owner, self.remediation
                ))
            })
            .collect::<Vec<_>>();
        let relative = path.to_string_lossy().replace('\\', "/");
        let approved_policy = self
            .approved_language_policy
            .iter()
            .find(|(approved, _)| relative.ends_with(approved));
        if let Some((_, reason)) = approved_policy {
            assert!(
                !reason.is_empty(),
                "approved language policy needs a reason"
            );
        } else {
            findings.extend(all_structs(source).into_iter().filter_map(|item| {
                let fields = field_names(&item);
                let storage = fields
                    .iter()
                    .find(|field| self.private_graph_storage_fields.contains(field))?;
                let edge = fields
                    .iter()
                    .find(|field| self.private_graph_edge_fields.contains(field))?;
                Some(format!(
                    "{} defines private capture graph fields `{storage}` and `{edge}`; owner: {}; remediation: {}",
                    path.display(), self.owner, self.remediation
                ))
            }));
        }
        let lower = source.to_ascii_lowercase();
        if lower.contains("functioninstance") {
            for exclusion in &self.non_participants {
                if exclusion.terms.iter().any(|term| lower.contains(term)) {
                    findings.push(format!(
                        "{} recasts {} as FunctionInstance [{}]: {}",
                        path.display(),
                        exclusion.model,
                        exclusion.mismatch_code,
                        exclusion.reason
                    ));
                }
            }
        }
        findings.sort();
        findings.dedup();
        findings
    }
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

fn public_structs(source: &str) -> Vec<String> {
    structs(source, true)
}

fn all_structs(source: &str) -> Vec<String> {
    structs(source, false)
}

fn structs(source: &str, public_only: bool) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = None::<(String, i32)>;
    for line in source.lines() {
        let trimmed = line.trim_start();
        let starts_struct = trimmed.starts_with("struct ") || trimmed.starts_with("pub struct ");
        if current.is_none()
            && starts_struct
            && (!public_only || trimmed.starts_with("pub struct "))
        {
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
    let mut path = env::current_dir().expect("test working directory must exist");
    loop {
        if path.join("function-ownership.toml").is_file() {
            return path;
        }
        assert!(path.pop(), "function ownership policy repository not found");
    }
}

#[test]
fn guard_rejects_shadow_schema_and_admits_language_policy() {
    let root = repository_root();
    let policy = Policy::load(&root);
    let findings = policy.findings(
        Path::new("crates/sim-lib-lang-example/src/function.rs"),
        SHADOW_FUNCTION,
    );
    assert_eq!(findings.len(), 1);
    assert!(findings[0].contains(&policy.owner));
    assert!(findings[0].contains(&policy.remediation));
    assert!(
        policy
            .findings(Path::new("frame.rs"), LANGUAGE_POLICY_ONLY)
            .is_empty()
    );
}

#[test]
fn guard_rejects_private_capture_graph() {
    let root = repository_root();
    let policy = Policy::load(&root);
    let findings = policy.findings(Path::new("private_environment.rs"), PRIVATE_CAPTURE_GRAPH);
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("private capture graph"))
    );
}

#[test]
fn guard_rejects_dictionary_function_adapter_but_admits_managed_edges() {
    let root = repository_root();
    let policy = Policy::load(&root);
    let findings = policy.findings(
        Path::new("dictionary_adapter.rs"),
        DICTIONARY_FUNCTION_ADAPTER,
    );
    assert!(findings.iter().any(|finding| {
        finding.contains("typeclass-dictionary")
            && finding.contains("dictionary-is-constraint-evidence-not-callable-identity")
    }));
    assert!(
        policy
            .findings(Path::new("dictionary.rs"), DICTIONARY_MANAGED_EDGES)
            .is_empty()
    );
    let prolog = policy
        .non_participants
        .iter()
        .find(|entry| entry.model == "prolog-predicate")
        .expect("Prolog must be classified in the function exclusion ledger");
    assert!(!prolog.reason.is_empty());
    assert_eq!(prolog.approved_relationships, ["binding", "shape"]);
    for entry in &policy.non_participants {
        assert!(
            !entry.model.is_empty()
                && !entry.terms.is_empty()
                && !entry.mismatch_code.is_empty()
                && !entry.reason.is_empty()
        );
    }
}

#[test]
fn guest_crates_have_no_unclassified_shadow_schema() {
    let root = repository_root();
    let policy = Policy::load(&root);
    for entry in fs::read_dir(root.join("crates")).unwrap() {
        let entry = entry.unwrap();
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(&policy.guest_crate_prefix)
        {
            continue;
        }
        let mut paths = vec![entry.path().join("src")];
        while let Some(path) = paths.pop() {
            for child in fs::read_dir(path).unwrap() {
                let child = child.unwrap();
                if child.file_type().unwrap().is_dir() {
                    paths.push(child.path());
                } else if child.path().extension().is_some_and(|ext| ext == "rs") {
                    let source = fs::read_to_string(child.path()).unwrap();
                    let findings = policy.findings(&child.path(), &source);
                    assert!(findings.is_empty(), "{}", findings.join("\n"));
                }
            }
        }
    }
}

#[test]
fn migrated_overlap_board_has_no_unclassified_cell() {
    let root = repository_root();
    let board = fs::read_to_string(root.join("unify-ownership-board.toml")).unwrap();
    assert_eq!(scalar(&board, "schema"), "sim.unify-ownership-board/v1");
    let concerns = array(&board, "concerns");
    assert_eq!(
        concerns,
        [
            "function-plan",
            "capture-graph",
            "class-descriptor",
            "lineage"
        ]
    );
    let rows = sections(&board, "[[language]]").collect::<Vec<_>>();
    assert_eq!(rows.len(), 6);
    for row in rows {
        let name = scalar(row, "name");
        let cells = array(row, "cells");
        assert!(!name.is_empty());
        assert_eq!(
            cells.len(),
            concerns.len(),
            "{name} has an incomplete overlap row"
        );
        assert!(
            cells
                .iter()
                .all(|cell| matches!(cell.as_str(), "shared" | "language-policy" | "absent"))
        );
    }
    for (path, reason) in &Policy::load(&root).approved_language_policy {
        assert!(!path.is_empty() && !reason.is_empty());
    }
}
