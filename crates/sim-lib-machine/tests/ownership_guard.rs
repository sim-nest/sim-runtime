// conformance: guest instruction semantics remain outside the shared machine organ.

//! Source-fact ownership guard for guest instruction semantics.

use std::{fs, path::Path};

use toml::Value;

const BAD_GUEST: &str = r#"
pub struct LuaFrameStack { frames: Vec<LuaFrame> }
pub fn interpret(code: &[Opcode]) { interpret(code) }
"#;

const INSTRUCTION_ONLY_GUEST: &str = r#"
pub enum LuaOpcode { Constant, Add, Return }
pub fn execute_instruction(opcode: LuaOpcode) { let _ = opcode; }
"#;

#[derive(Debug)]
struct Policy {
    forbidden_types: Vec<String>,
    recursive_drivers: Vec<String>,
    allowed_types: Vec<String>,
    allowed_functions: Vec<String>,
}

impl Policy {
    fn load(root: &Path) -> Self {
        let source = fs::read_to_string(root.join("machine-ownership.toml")).unwrap();
        let value = source.parse::<Value>().unwrap();
        assert_eq!(value["schema"].as_str(), Some("sim.machine-ownership/v1"));
        Self {
            forbidden_types: strings(&value, "forbidden_public_type_suffixes"),
            recursive_drivers: strings(&value, "forbidden_recursive_driver_names"),
            allowed_types: strings(&value, "allowed_instruction_type_suffixes"),
            allowed_functions: strings(&value, "allowed_instruction_function_prefixes"),
        }
    }

    fn findings(&self, source: &str) -> Vec<String> {
        let mut findings = Vec::new();
        for line in source.lines().map(str::trim) {
            if let Some(name) = public_type_name(line) {
                let allowed = self
                    .allowed_types
                    .iter()
                    .any(|suffix| name.ends_with(suffix));
                if !allowed
                    && self
                        .forbidden_types
                        .iter()
                        .any(|suffix| name.ends_with(suffix))
                {
                    findings.push(format!("guest-owned machine type {name}"));
                }
            }
            if let Some(name) = function_name(line) {
                let allowed = self
                    .allowed_functions
                    .iter()
                    .any(|prefix| name.starts_with(prefix));
                let driver = self
                    .recursive_drivers
                    .iter()
                    .any(|candidate| candidate == name);
                if !allowed && driver && function_calls_itself(source, name) {
                    findings.push(format!("recursive guest interpreter loop {name}"));
                }
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

fn public_type_name(line: &str) -> Option<&str> {
    ["pub struct ", "pub enum ", "pub trait ", "pub type "]
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
    rest.split_once('(').map(|(name, _)| name.trim())
}

fn function_calls_itself(source: &str, name: &str) -> bool {
    let declaration = format!("fn {name}(");
    let call = format!("{name}(");
    source.matches(&call).count() > source.matches(&declaration).count()
}

#[test]
fn guest_machine_ownership_policy_rejects_machine_and_allows_instruction_semantics() {
    let mut root = fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("src"))
        .expect("machine source directory must resolve");
    while !root.join("machine-ownership.toml").is_file() {
        assert!(root.pop(), "machine ownership repository not found");
    }
    let policy = Policy::load(&root);

    let bad = policy.findings(BAD_GUEST);
    assert!(bad.iter().any(|finding| finding.contains("LuaFrameStack")));
    assert!(bad.iter().any(|finding| finding.contains("interpret")));
    assert!(policy.findings(INSTRUCTION_ONLY_GUEST).is_empty());

    for entry in fs::read_dir(root.join("crates")).unwrap() {
        let entry = entry.unwrap();
        let crate_name = entry.file_name();
        if !crate_name.to_string_lossy().starts_with("sim-lib-lang-") {
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
                    assert!(
                        policy.findings(&source).is_empty(),
                        "{} violates machine ownership: {:?}",
                        child.path().display(),
                        policy.findings(&source)
                    );
                }
            }
        }
    }
}
