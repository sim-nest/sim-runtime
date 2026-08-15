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
    legacy_paths: Vec<(String, String)>,
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
        let legacy_paths = source
            .split("[[legacy_migration]]")
            .skip(1)
            .map(|row| {
                let path = scalar(row, "path");
                let reason = scalar(row, "reason");
                assert!(!path.is_empty() && !reason.is_empty());
                (path, reason)
            })
            .collect();
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
            legacy_paths,
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

    fn legacy_reason(&self, path: &str) -> Option<&str> {
        self.legacy_paths
            .iter()
            .find(|(candidate, _)| candidate == path)
            .map(|(_, reason)| reason.as_str())
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
                    let relative = child
                        .path()
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    if let Some(reason) = policy.legacy_reason(&relative) {
                        assert!(!reason.is_empty());
                        continue;
                    }
                    let source = fs::read_to_string(child.path()).unwrap();
                    let findings = policy.findings(&child.path(), &source);
                    assert!(findings.is_empty(), "{}", findings.join("\n"));
                }
            }
        }
    }
}
