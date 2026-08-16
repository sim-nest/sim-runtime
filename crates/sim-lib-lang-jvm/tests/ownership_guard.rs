//! Policy-driven source-fact guard for JVM organ ownership.

use std::{fs, path::Path};

use toml::Value;

const VIOLATIONS: &[(&str, &str)] = &[
    ("classfile parsing", "pub struct ClassfileParser;"),
    ("opcode list", "pub struct JvmOpcodeList;"),
    ("private stack", "pub struct OperandStack;"),
    ("private frame", "pub struct PrivateFrame;"),
    (
        "raw UTF-16 wrapper",
        "pub struct JavaString { units: Vec<u16> }",
    ),
    ("shadow managed graph", "pub struct ShadowManagedGraph;"),
    ("ambient classpath", "pub struct Classpath;"),
    (
        "string class matching",
        "fn select(class_name: String) { match class_name.as_str() { _ => {} } }",
    ),
    ("unwind payload", "pub struct JvmUnwindPayload;"),
    ("alternate entry", "pub fn enter_method() {}"),
    ("alternate drive", "pub fn drive_method() {}"),
];

#[derive(Debug)]
struct Policy {
    type_suffixes: Vec<String>,
    function_names: Vec<String>,
    field_types: Vec<String>,
    source_patterns: Vec<String>,
    allowed_drive: String,
}

impl Policy {
    fn load(root: &Path) -> Self {
        let value: Value = fs::read_to_string(root.join("jvm-ownership.toml"))
            .unwrap()
            .parse()
            .unwrap();
        assert_eq!(value["schema"].as_str(), Some("sim.jvm-ownership/v1"));
        Self {
            type_suffixes: strings(&value, "forbidden_type_suffixes"),
            function_names: strings(&value, "forbidden_function_names"),
            field_types: strings(&value, "forbidden_field_types"),
            source_patterns: strings(&value, "forbidden_source_patterns"),
            allowed_drive: value["allowed_public_drive"].as_str().unwrap().into(),
        }
    }

    fn findings(&self, source: &str) -> Vec<String> {
        let mut findings = Vec::new();
        for line in source.lines().map(str::trim) {
            if let Some(name) = declaration_name(line)
                && self
                    .type_suffixes
                    .iter()
                    .any(|suffix| name.ends_with(suffix))
            {
                findings.push(format!("forbidden JVM-owned type {name}"));
            }
            if let Some(name) = function_name(line) {
                if self
                    .function_names
                    .iter()
                    .any(|candidate| candidate == name)
                {
                    findings.push(format!("alternate JVM entry {name}"));
                }
                if line.starts_with("pub fn drive") && name != self.allowed_drive {
                    findings.push(format!("alternate JVM drive {name}"));
                }
            }
            for field_type in &self.field_types {
                let tuple_wrapper = line.contains('(') && line.ends_with("),");
                let record_wrapper =
                    declaration_name(line).is_some() && line.contains('{') && line.contains(':');
                if (tuple_wrapper || record_wrapper) && line.contains(field_type) {
                    findings.push(format!("forbidden JVM-owned field type {field_type}"));
                }
            }
        }
        for pattern in &self.source_patterns {
            if source.contains(pattern) {
                findings.push(format!("forbidden JVM source fact {pattern}"));
            }
        }
        findings.sort();
        findings.dedup();
        findings
    }
}

fn strings(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item.as_str().unwrap().to_owned())
        .collect()
}

fn declaration_name(line: &str) -> Option<&str> {
    [
        "pub struct ",
        "pub enum ",
        "pub trait ",
        "pub type ",
        "struct ",
        "enum ",
    ]
    .into_iter()
    .find_map(|prefix| line.strip_prefix(prefix))
    .and_then(|rest| {
        rest.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
            .next()
    })
}

fn function_name(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix("pub fn ")
        .or_else(|| line.strip_prefix("fn "))?;
    rest.split_once('(')
        .map(|(name, _)| name.split('<').next().unwrap().trim())
}

fn rust_sources(path: &Path, output: &mut Vec<std::path::PathBuf>) {
    for child in fs::read_dir(path).unwrap() {
        let child = child.unwrap();
        if child.file_type().unwrap().is_dir() {
            rust_sources(&child.path(), output);
        } else if child.path().extension().is_some_and(|ext| ext == "rs") {
            output.push(child.path());
        }
    }
}

#[test]
fn every_ownership_guard_has_a_violating_fixture() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let policy = Policy::load(root);
    for (name, fixture) in VIOLATIONS {
        assert!(
            !policy.findings(fixture).is_empty(),
            "{name} fixture was not rejected"
        );
    }
}

#[test]
fn jvm_sources_contain_no_organ_ownership_forks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let policy = Policy::load(root);
    let mut paths = Vec::new();
    rust_sources(&root.join("src"), &mut paths);
    for path in paths {
        let source = fs::read_to_string(&path).unwrap();
        assert!(
            policy.findings(&source).is_empty(),
            "{} violates JVM organ ownership: {:?}",
            path.display(),
            policy.findings(&source)
        );
    }
}
