//! Structural ownership guard for exact UTF-16 text.

use std::{
    fs,
    path::{Path, PathBuf},
};

const DUPLICATE_TEXT: &str = r#"
pub struct JavascriptCodeUnitString {
    units: Vec<u16>,
}
"#;
const LOSSY_CROSSING: &str = r#"
pub fn display(units: &[u16]) -> String { String::from_utf16_lossy(units) }
"#;
const AUDIO_BUFFER: &str = r#"
pub struct AudioSamples {
    samples: Vec<u16>,
}
"#;
const SHARED_TEXT_WRAPPER: &str = r#"
pub struct JavascriptCodeUnitString(CodeUnitString);
impl JavascriptCodeUnitString {
    pub fn from_code_units(units: Vec<u16>) -> Self {
        Self(CodeUnitString::from_code_units(units))
    }
}
"#;

#[derive(Debug)]
struct Context {
    path: String,
    class: String,
    reason: String,
}

#[derive(Debug)]
struct Policy {
    owner: String,
    remediation: String,
    contexts: Vec<Context>,
}

impl Policy {
    fn load(root: &Path) -> Self {
        let source = fs::read_to_string(root.join("text-ownership.toml"))
            .expect("text ownership policy must exist");
        assert_eq!(scalar(&source, "schema"), "sim.text-ownership/v1");
        Self {
            owner: scalar(&source, "owner"),
            remediation: scalar(&source, "remediation"),
            contexts: sections(&source, "[[approved_context]]")
                .map(|row| Context {
                    path: scalar(row, "path"),
                    class: scalar(row, "class"),
                    reason: scalar(row, "reason"),
                })
                .collect(),
        }
    }

    fn findings(&self, path: &Path, source: &str) -> Vec<String> {
        let relative = path.to_string_lossy().replace('\\', "/");
        let mut findings = Vec::new();
        if source.contains("from_utf16_lossy") {
            findings.push(format!("{relative} crosses exact UTF-16 through lossy scalar text; owner: {}; remediation: {}", self.owner, self.remediation));
        }
        if let Some(context) = self
            .contexts
            .iter()
            .find(|row| relative.ends_with(&row.path))
        {
            assert!(matches!(
                context.class.as_str(),
                "numeric-set" | "codec-work-array"
            ));
            assert!(!context.reason.is_empty());
            return findings;
        }
        for item in structs(source) {
            let declaration = item.lines().next().unwrap_or_default().trim_start();
            if declaration.starts_with("pub struct ")
                && item.contains("Vec<u16>")
                && looks_like_text(&item)
            {
                findings.push(format!("{relative} declares a public raw Vec<u16> text record; owner: {}; remediation: {}", self.owner, self.remediation));
            }
        }
        findings
    }
}

fn looks_like_text(item: &str) -> bool {
    let lower = item.to_ascii_lowercase();
    ["string", "text", "utf16", "codeunit", "code_unit"]
        .iter()
        .any(|term| lower.contains(term))
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
        .find_map(|line| {
            line.strip_prefix(&format!("{key} = \""))
                .and_then(|rest| rest.strip_suffix('"'))
        })
        .unwrap_or_default()
        .to_owned()
}
fn structs(source: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = None::<(String, i32)>;
    for line in source.lines() {
        let trimmed = line.trim_start();
        if current.is_none()
            && (trimmed.starts_with("struct ") || trimmed.starts_with("pub struct "))
        {
            let text = format!("{line}\n");
            let depth = brace_delta(line);
            if depth <= 0 {
                result.push(text);
            } else {
                current = Some((text, depth));
            }
            continue;
        }
        if let Some((text, depth)) = &mut current {
            text.push_str(line);
            text.push('\n');
            *depth += brace_delta(line);
            if *depth <= 0 {
                result.push(std::mem::take(text));
                current = None;
            }
        }
    }
    result
}
fn brace_delta(line: &str) -> i32 {
    line.bytes()
        .map(|b| match b {
            b'{' => 1,
            b'}' => -1,
            _ => 0,
        })
        .sum()
}
fn repository_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .canonicalize()
        .expect("JavaScript source directory must resolve");
    while !path.join("text-ownership.toml").is_file() {
        assert!(path.pop(), "text ownership repository not found");
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
fn fixtures_distinguish_owned_text_from_numeric_buffers() {
    let policy = Policy::load(&repository_root());
    assert!(
        policy
            .findings(Path::new("crates/example/src/text.rs"), DUPLICATE_TEXT)
            .iter()
            .any(|finding| finding.contains("JavascriptCodeUnitString")
                || finding.contains("public raw Vec<u16> text record"))
    );
    assert!(
        policy
            .findings(Path::new("crates/example/src/decode.rs"), LOSSY_CROSSING)
            .iter()
            .any(|finding| finding.contains("lossy"))
    );
    assert!(
        policy
            .findings(Path::new("crates/example/src/audio.rs"), AUDIO_BUFFER)
            .is_empty()
    );
    assert!(
        policy
            .findings(Path::new("crates/example/src/text.rs"), SHARED_TEXT_WRAPPER)
            .is_empty()
    );
}

#[test]
fn product_sources_respect_text_ownership() {
    let root = repository_root();
    let policy = Policy::load(&root);
    for path in rust_sources(&root.join("crates")) {
        if path.components().any(|part| part.as_os_str() == "tests")
            || path.ends_with("src/tests.rs")
        {
            continue;
        }
        let findings = policy.findings(&path, &fs::read_to_string(&path).unwrap());
        assert!(findings.is_empty(), "{}", findings.join("\n"));
    }
}

#[test]
fn every_exception_is_explicit_and_live() {
    let root = repository_root();
    let policy = Policy::load(&root);
    assert!(!policy.contexts.is_empty());
    for context in policy.contexts {
        assert!(
            root.join(&context.path).is_file(),
            "missing declared context {}",
            context.path
        );
    }
}
