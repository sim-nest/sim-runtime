//! Source-fact ownership guard for the single shared matcher organ.

use std::{
    fs,
    path::{Path, PathBuf},
};

const PRIVATE_MATCHER: &str = r#"
pub struct GuestNfa {
    states: Vec<State>,
    transitions: Vec<Transition>,
}
fn match_here(input: &[u8]) { match_here(&input[1..]) }
"#;

const SYNTAX_PARSER: &str = r#"
pub struct GuestPatternParser { cursor: usize }
pub fn parse_pattern(source: &str) -> PatternProgram { lower(source) }
"#;

#[derive(Debug)]
struct Relationship {
    path: String,
    class: String,
    reason: String,
}

#[derive(Debug)]
struct Policy {
    owner: String,
    remediation: String,
    guest_crate_prefix: String,
    roadmap_family_terms: Vec<String>,
    state_fields: Vec<String>,
    transition_fields: Vec<String>,
    driver_names: Vec<String>,
    relationships: Vec<Relationship>,
}

impl Policy {
    fn load(root: &Path) -> Self {
        let source = fs::read_to_string(root.join("pattern-ownership.toml"))
            .expect("pattern ownership policy must exist");
        assert_eq!(scalar(&source, "schema"), "sim.pattern-ownership/v1");
        Self {
            owner: scalar(&source, "owner"),
            remediation: scalar(&source, "remediation"),
            guest_crate_prefix: scalar(&source, "guest_crate_prefix"),
            roadmap_family_terms: array(&source, "roadmap_family_terms"),
            state_fields: array(&source, "state_fields"),
            transition_fields: array(&source, "transition_fields"),
            driver_names: array(&source, "driver_names"),
            relationships: sections(&source, "[[approved_relationship]]")
                .map(|row| Relationship {
                    path: scalar(row, "path"),
                    class: scalar(row, "class"),
                    reason: scalar(row, "reason"),
                })
                .collect(),
        }
    }

    fn findings(&self, path: &Path, source: &str) -> Vec<String> {
        let relative = path.to_string_lossy().replace('\\', "/");
        let relationship = self
            .relationships
            .iter()
            .find(|row| relative.ends_with(&row.path));
        if let Some(row) = relationship {
            assert!(matches!(
                row.class.as_str(),
                "syntax-parser" | "policy-adapter"
            ));
            assert!(!row.reason.is_empty());
        }
        let mut findings = structs(source).into_iter().filter_map(|item| {
            let fields = field_names(&item);
            let state = fields.iter().find(|field| self.state_fields.contains(field))?;
            let transition = fields.iter().find(|field| self.transition_fields.contains(field))?;
            Some(format!("{} retains private matcher fields `{state}` and `{transition}`; owner: {}; remediation: {}", relative, self.owner, self.remediation))
        }).collect::<Vec<_>>();
        for name in &self.driver_names {
            if function_calls_itself(source, name)
                || (name == "backtrack" && source.contains("backtrack_stack"))
            {
                findings.push(format!(
                    "{relative} declares guest matcher driver `{name}`; owner: {}",
                    self.owner
                ));
            }
        }
        for line in source.lines().filter(|line| is_public_diagnostic(line)) {
            if let Some(term) = self
                .roadmap_family_terms
                .iter()
                .find(|term| line.contains(term.as_str()))
            {
                findings.push(format!(
                    "{relative} leaks roadmap family `{term}` in a public diagnostic"
                ));
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
fn structs(source: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut current = None::<(String, i32)>;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if current.is_none()
            && (trimmed.starts_with("struct ") || trimmed.starts_with("pub struct "))
        {
            let depth = brace_delta(line);
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
fn function_calls_itself(source: &str, name: &str) -> bool {
    let declaration = format!("fn {name}(");
    let call = format!("{name}(");
    source.matches(&call).count() > source.matches(&declaration).count()
}
fn is_public_diagnostic(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    line.contains('"')
        && ["diagnostic", "error::", "error(", "format!(", "message"]
            .iter()
            .any(|term| lower.contains(term))
}
fn repository_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    while !path.join("pattern-ownership.toml").is_file() {
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
fn guard_rejects_private_matcher_and_admits_syntax_parser() {
    let policy = Policy::load(&repository_root());
    let findings = policy.findings(
        Path::new("crates/sim-lib-lang-example/src/matcher.rs"),
        PRIVATE_MATCHER,
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("private matcher"))
    );
    assert!(
        findings
            .iter()
            .any(|finding| finding.contains("match_here"))
    );
    assert!(
        policy
            .findings(
                Path::new("crates/sim-lib-lang-example/src/parser.rs"),
                SYNTAX_PARSER
            )
            .is_empty()
    );
}

#[test]
fn guest_crates_have_no_private_matcher_or_roadmap_diagnostic() {
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
        for path in rust_sources(&entry.path().join("src")) {
            let source = fs::read_to_string(&path).unwrap();
            let findings = policy.findings(&path, &source);
            assert!(findings.is_empty(), "{}", findings.join("\n"));
        }
    }
}

#[test]
fn pattern_overlap_board_is_fully_classified() {
    let root = repository_root();
    let source = fs::read_to_string(root.join("pattern-ownership.toml")).unwrap();
    let rows = sections(&source, "[[overlap]]").collect::<Vec<_>>();
    assert!(!rows.is_empty());
    for row in rows {
        let name = scalar(row, "name");
        let path = scalar(row, "path");
        let class = scalar(row, "class");
        let reason = scalar(row, "reason");
        assert!(!name.is_empty() && !path.is_empty() && !reason.is_empty());
        assert!(
            matches!(
                class.as_str(),
                "shared-owner" | "syntax-parser" | "policy-adapter"
            ),
            "{name} is unclassified"
        );
        assert!(
            root.join(path).is_file(),
            "{name} points at a missing source fact"
        );
    }
}
