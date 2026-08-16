const OWNERSHIP: &str = include_str!("fixtures/exceptions3_carrier_ownership.tsv");
const SCENARIOS: &str = include_str!("fixtures/exceptions3_characterize_1.tsv");
const POLICY: &str = include_str!("../../../exception-ownership.toml");

#[derive(Debug, Eq, PartialEq)]
struct Record {
    name: String,
    fields: Vec<(String, String)>,
}

fn records(source: &str) -> Vec<Record> {
    let mut found = Vec::new();
    let mut lines = source.lines();
    while let Some(line) = lines.next() {
        let Some(after) = line
            .trim_start()
            .strip_prefix("pub struct ")
            .or_else(|| line.trim_start().strip_prefix("struct "))
        else {
            continue;
        };
        let name = after
            .split(|ch: char| ch == '<' || ch == '{' || ch.is_whitespace())
            .next()
            .unwrap()
            .to_owned();
        if !line.contains('{') {
            continue;
        }
        let mut fields = Vec::new();
        for field_line in lines.by_ref() {
            let field_line = field_line.trim();
            if field_line.starts_with('}') {
                break;
            }
            let field_line = field_line.strip_prefix("pub ").unwrap_or(field_line);
            let Some((field, ty)) = field_line.split_once(':') else {
                continue;
            };
            let field = field.trim();
            if field
                .chars()
                .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
            {
                fields.push((field.to_owned(), ty.trim().trim_end_matches(',').to_owned()));
            }
        }
        found.push(Record { name, fields });
    }
    found
}

fn array(policy: &str, key: &str) -> Vec<String> {
    let prefix = format!("{key} = [");
    let line = policy
        .lines()
        .find(|line| line.starts_with(&prefix))
        .unwrap();
    line[prefix.len()..]
        .trim_end_matches(']')
        .split(',')
        .map(|item| item.trim().trim_matches('"').to_owned())
        .collect()
}

fn scalar_from(block: &str, key: &str) -> Option<String> {
    let prefix = format!("{key} = \"");
    block
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(|value| value.trim_end_matches('"').to_owned())
}

fn approved(path: &str, record: &Record) -> bool {
    POLICY.split("[[guest_object]]").skip(1).any(|block| {
        scalar_from(block, "path").as_deref() == Some(path)
            && scalar_from(block, "record").as_deref() == Some(record.name.as_str())
            && {
                let allowed = array(block, "fields");
                record
                    .fields
                    .iter()
                    .all(|(field, _)| allowed.contains(field))
            }
    })
}

fn violations(path: &str, source: &str) -> Vec<String> {
    let class_fields = array(POLICY, "class_fields");
    let payload_fields = array(POLICY, "payload_fields");
    let containers = array(POLICY, "relation_containers");
    let record_terms = array(POLICY, "relation_record_terms");
    records(source)
        .into_iter()
        .filter(|record| !approved(path, record))
        .filter_map(|record| {
            let names = record
                .fields
                .iter()
                .map(|(name, _)| name)
                .collect::<Vec<_>>();
            let record_like = record_terms.iter().any(|term| record.name.contains(term));
            let has_class = names.iter().any(|name| {
                class_fields.contains(name)
                    && (name.contains("class") || (record_like && name.contains("name")))
            });
            let has_payload = names.iter().any(|name| {
                payload_fields.contains(name)
                    && (matches!(name.as_str(), "message" | "payload") || record_like)
            });
            let copied_carrier = has_class && has_payload;
            let relation_chain = record.fields.iter().any(|(_, ty)| {
                containers
                    .iter()
                    .any(|container| ty.contains(&format!("{container}<")))
                    && record_terms.iter().any(|term| ty.contains(term))
            });
            (copied_carrier || relation_chain).then(|| {
                format!(
                    "{path}::{} copies the raised carrier or relation chain",
                    record.name
                )
            })
        })
        .collect()
}

#[test]
fn control_carriers_and_guest_fields_have_explicit_non_recursive_ownership() {
    let mut modules = std::collections::BTreeSet::new();
    for (line_number, line) in OWNERSHIP.lines().enumerate().skip(1) {
        let cells = line.split('\t').collect::<Vec<_>>();
        assert_eq!(cells.len(), 5, "invalid ownership row {}", line_number + 1);
        assert!(matches!(cells[3], "envelope" | "guest-object" | "delete"));
        if cells[3] == "envelope" {
            assert!(
                !cells[2].contains("Raised"),
                "envelope recursively owns a raised carrier: {line}"
            );
        }
        if let Some(module) = cells[0].strip_prefix("control::") {
            modules.insert(module.split(['<', '(']).next().unwrap());
        }
    }
    assert_eq!(
        modules,
        [
            "Condition",
            "ProtectedOutcome",
            "Restart",
            "ResumePacket",
            "ResumeResult",
            "Unwind",
            "run_with_close_guards",
        ]
        .into_iter()
        .collect()
    );
}

fn content_id(value: &str) -> u64 {
    value.bytes().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    })
}

#[test]
fn characterize_1_manifest_is_complete_and_replays_identically() {
    let rows = SCENARIOS.lines().skip(1).collect::<Vec<_>>();
    for required in [
        "raise-catch-class",
        "catch-superclass",
        "no-match-propagation",
        "explicit-cause",
        "implicit-context",
        "suppression",
        "group-construction-split",
        "aggregate-error-order",
        "arbitrary-non-object-throw",
        "cleanup-order-under-unwind",
        "resume-after-protected-call",
    ] {
        assert!(
            rows.iter()
                .any(|row| row.split('\t').nth(1) == Some(required))
        );
    }
    assert!(rows.iter().all(|row| row.split('\t').count() == 4));
    let first = rows.iter().map(|row| content_id(row)).collect::<Vec<_>>();
    let replay = rows.iter().map(|row| content_id(row)).collect::<Vec<_>>();
    assert_eq!(first, replay);
}

#[test]
fn source_fact_guard_rejects_forked_carriers_and_admits_declared_guest_objects() {
    assert!(
        violations(
            "crates/sim-lib-lang-javascript/src/error.rs",
            "pub struct JavascriptException {\n pub name: String,\n pub message: String,\n}",
        )
        .iter()
        .any(|finding| finding.contains("JavascriptException"))
    );
    assert!(
        violations(
            "crates/sim-lib-lang-jvm/src/throwable.rs",
            "struct JvmThrowableRecord {\n class: ClassRef,\n message: String,\n}",
        )
        .iter()
        .any(|finding| finding.contains("JvmThrowableRecord"))
    );
    assert!(
        violations(
            "crates/sim-lib-lang-jvm/src/throwable.rs",
            "struct ThrowableChain {\n causes: Vec<JvmThrowableRecord>,\n}",
        )
        .iter()
        .any(|finding| finding.contains("ThrowableChain"))
    );

    assert!(
        violations(
            "crates/sim-lib-lang-javascript/src/objects.rs",
            "pub struct JavascriptCallError {\n pub origin: String,\n pub message: String,\n}",
        )
        .is_empty()
    );
    assert!(violations(
        "crates/sim-lib-lang-python/src/resumable.rs",
        "pub struct PythonExceptionData {\n class: ClassRef,\n message: String,\n group_message: Option<String>,\n}",
    )
    .is_empty());
}

#[test]
fn runtime_sources_contain_no_undeclared_exception_carrier() {
    let current = std::env::var_os("SIM_RUNTIME_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap());
    let root = if current.join("exception-ownership.toml").is_file() {
        current
    } else {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    };
    let mut findings = Vec::new();
    for crate_entry in std::fs::read_dir(root.join("crates")).unwrap() {
        let crate_path = crate_entry.unwrap().path();
        if crate_path.ends_with("sim-lib-control") {
            continue;
        }
        let src = crate_path.join("src");
        if !src.is_dir() {
            continue;
        }
        let mut pending = vec![src];
        while let Some(directory) = pending.pop() {
            for entry in std::fs::read_dir(directory).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    pending.push(path);
                } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                    let relative = path
                        .strip_prefix(&root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    let source = std::fs::read_to_string(&path).unwrap();
                    findings.extend(violations(&relative, &source));
                }
            }
        }
    }
    assert!(findings.is_empty(), "{}", findings.join("\n"));
}
