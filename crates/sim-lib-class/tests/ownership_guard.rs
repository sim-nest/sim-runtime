//! Structural source-fact guard for the declared-class semantic boundary.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

const JAVASCRIPT_ADAPTER: &str = r#"
pub fn adapt(object: &JavascriptObject) -> ClassDescriptor {
    ClassDescriptor { parents: vec![object.prototype()], name: object.name(), members: vec![] }
}
"#;

const PYTHON_CLASS: &str = r#"
pub struct PythonClass { name: Symbol, bases: Vec<ClassRef> }
pub fn declared_parents(class: &PythonClass) -> &[ClassRef] { &class.bases }
"#;

const UNBOUNDED_LINEAGE: &str = r#"
pub fn ancestors(class: ClassRef) -> Vec<ClassRef> {
    let mut parents = vec![class];
    while let Some(parent) = parents.pop() { parents.extend(parent.parents()); }
    parents
}
"#;

#[derive(Debug)]
struct Exclusion {
    model: String,
    terms: Vec<String>,
    mismatch_code: String,
}

#[derive(Debug)]
struct Policy {
    owner: String,
    remediation: String,
    crate_prefix: String,
    descriptor_fields: Vec<String>,
    descriptor_minimum_fields: usize,
    lineage_terms: Vec<String>,
    bound_terms: Vec<String>,
    exclusions: Vec<Exclusion>,
    approved_paths: Vec<String>,
}

impl Policy {
    fn load(root: &Path) -> Self {
        let source = fs::read_to_string(root.join("class-ownership.toml"))
            .expect("class ownership policy must exist");
        assert_eq!(scalar(&source, "schema"), "sim.class-ownership/v1");
        let exclusions = sections(&source, "[[exclusion]]")
            .map(|row| Exclusion {
                model: scalar(row, "model"),
                terms: array(row, "source_terms"),
                mismatch_code: scalar(row, "mismatch_code"),
            })
            .collect::<Vec<_>>();
        assert!(!exclusions.is_empty());
        Self {
            owner: scalar(&source, "owner"),
            remediation: scalar(&source, "remediation"),
            crate_prefix: scalar(&source, "crate_prefix"),
            descriptor_fields: array(&source, "descriptor_fields"),
            descriptor_minimum_fields: integer(&source, "descriptor_minimum_fields"),
            lineage_terms: array(&source, "lineage_terms"),
            bound_terms: array(&source, "bound_terms"),
            exclusions,
            approved_paths: sections(&source, "[[approved_relationship]]")
                .map(|row| scalar(row, "path"))
                .collect(),
        }
    }

    fn findings(&self, path: &Path, source: &str) -> Vec<String> {
        let relative = path.to_string_lossy().replace('\\', "/");
        if self
            .approved_paths
            .iter()
            .any(|approved| relative.ends_with(approved))
        {
            return Vec::new();
        }
        let lower = source.to_ascii_lowercase();
        let mut findings = Vec::new();
        for item in public_structs(&lower) {
            let count = field_names(&item)
                .iter()
                .filter(|field| self.descriptor_fields.contains(field))
                .count();
            if count >= self.descriptor_minimum_fields {
                findings.push(format!(
                    "{} copies the declared class descriptor owned by {}; {}",
                    path.display(),
                    self.owner,
                    self.remediation
                ));
            }
        }
        for body in function_bodies(&lower) {
            let walks_lineage = self.lineage_terms.iter().any(|term| {
                if term == "parent" || term == "parents" {
                    body.contains(".parents(")
                        || body.contains("parents()")
                        || body.contains("declared_parents")
                } else {
                    body.contains(term)
                }
            });
            let loops = body.contains("while ") || body.contains("loop {") || body.contains("for ");
            let bounded = self.bound_terms.iter().any(|term| body.contains(term));
            if walks_lineage && loops && !bounded {
                findings.push(format!(
                    "{} contains an unbounded lineage walk; {}",
                    path.display(),
                    self.remediation
                ));
            }
            let declares_parents = body.contains("parents:")
                || body.contains("declared_parents")
                || body.contains("set_parents(");
            if declares_parents {
                for exclusion in &self.exclusions {
                    if exclusion.terms.iter().any(|term| body.contains(term)) {
                        findings.push(format!(
                            "{} recasts {} state as declared parents [{}]; {}",
                            path.display(),
                            exclusion.model,
                            exclusion.mismatch_code,
                            self.remediation
                        ));
                    }
                }
            }
        }
        findings.sort();
        findings.dedup();
        findings
    }
}

fn scalar(source: &str, key: &str) -> String {
    source
        .lines()
        .map(str::trim)
        .find_map(|line| {
            line.strip_prefix(&format!("{key} = \""))?
                .strip_suffix('"')
                .map(str::to_owned)
        })
        .unwrap_or_default()
}
fn integer(source: &str, key: &str) -> usize {
    source
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(&format!("{key} = "))?.parse().ok())
        .unwrap_or_default()
}
fn array(source: &str, key: &str) -> Vec<String> {
    let prefix = format!("{key} = [");
    source
        .lines()
        .map(str::trim)
        .find_map(|line| {
            let body = line.strip_prefix(&prefix)?.strip_suffix(']')?;
            Some(
                body.split(',')
                    .filter_map(|item| {
                        item.trim()
                            .strip_prefix('"')?
                            .strip_suffix('"')
                            .map(str::to_owned)
                    })
                    .collect(),
            )
        })
        .unwrap_or_default()
}
fn sections<'a>(source: &'a str, heading: &str) -> impl Iterator<Item = &'a str> {
    source
        .split(heading)
        .skip(1)
        .map(|row| row.split("[[").next().unwrap_or(row))
}

fn public_structs(source: &str) -> Vec<String> {
    braced_items(source, "pub struct ")
}
fn function_bodies(source: &str) -> Vec<String> {
    let mut bodies = braced_items(source, "fn ");
    bodies.extend(braced_items(source, "pub fn "));
    bodies
}
fn braced_items(source: &str, marker: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut offset = 0;
    while let Some(found) = source[offset..].find(marker) {
        let start = offset + found;
        let Some(open_rel) = source[start..].find('{') else {
            break;
        };
        let open = start + open_rel;
        let mut depth = 0_i32;
        for (rel, byte) in source[open..].bytes().enumerate() {
            depth += match byte {
                b'{' => 1,
                b'}' => -1,
                _ => 0,
            };
            if depth == 0 {
                out.push(source[start..=open + rel].to_owned());
                offset = open + rel + 1;
                break;
            }
        }
        if offset <= start {
            break;
        }
    }
    out
}
fn field_names(item: &str) -> Vec<String> {
    item.lines()
        .skip(1)
        .filter_map(|line| {
            line.trim()
                .trim_start_matches("pub ")
                .split_once(':')
                .map(|(name, _)| name.trim().to_owned())
        })
        .collect()
}
fn repository_root() -> PathBuf {
    let mut path = env::current_dir().unwrap();
    while !path.join("class-ownership.toml").is_file() {
        assert!(path.pop());
    }
    path
}

#[test]
fn policy_is_ledger_driven_and_distinguishes_prototypes_from_real_parents() {
    let root = repository_root();
    let policy = Policy::load(&root);
    let bad = policy.findings(Path::new("adapter.rs"), JAVASCRIPT_ADAPTER);
    assert!(
        bad.iter()
            .any(|finding| finding.contains("javascript-prototype")
                && finding.contains("prototype-delegates-properties"))
    );
    assert!(
        policy
            .findings(Path::new("python.rs"), PYTHON_CLASS)
            .is_empty()
    );
    assert!(
        policy
            .findings(Path::new("walk.rs"), UNBOUNDED_LINEAGE)
            .iter()
            .any(|finding| finding.contains("unbounded lineage"))
    );
    for exclusion in &policy.exclusions {
        assert!(
            !exclusion.model.is_empty()
                && !exclusion.mismatch_code.is_empty()
                && !exclusion.terms.is_empty()
        );
    }
}

#[test]
fn repository_has_no_unapproved_class_boundary_violation() {
    let root = repository_root();
    let policy = Policy::load(&root);
    for entry in fs::read_dir(root.join("crates")).unwrap().flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(&policy.crate_prefix)
        {
            continue;
        }
        let mut paths = vec![entry.path().join("src")];
        while let Some(path) = paths.pop() {
            let Ok(children) = fs::read_dir(path) else {
                continue;
            };
            for child in children.flatten() {
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
