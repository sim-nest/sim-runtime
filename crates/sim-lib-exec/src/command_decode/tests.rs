use super::*;
use crate::{SandboxCpuLimit, SandboxFilesystemLimit, SandboxMemoryLimit};

fn specimen(mask: u8) -> CommandSpec {
    let mut limits = crate::sandbox_limit_tests::limits();
    if mask & 1 != 0 {
        limits.cpu = SandboxCpuLimit::Rate {
            quota_us: 200_000,
            period_us: 100_000,
        };
    }
    if mask & 2 != 0 {
        limits.memory = SandboxMemoryLimit::Charged {
            bytes: 4096,
            swap_bytes: 0,
        };
    }
    if mask & 4 != 0 {
        limits.filesystem = SandboxFilesystemLimit::Allocated {
            inodes: 8,
            bytes: 4096,
        };
    }
    crate::sandbox_limit_tests::command(limits)
}

fn field_mut<'a>(value: &'a mut Datum, name: &str) -> &'a mut Datum {
    let Datum::Node { fields, .. } = value else {
        panic!("node")
    };
    &mut fields
        .iter_mut()
        .find(|(key, _)| key == &Symbol::new(name))
        .unwrap()
        .1
}

fn reordered(value: &mut Datum) {
    match value {
        Datum::Node { fields, .. } => {
            fields.reverse();
            for (_, value) in fields {
                reordered(value);
            }
        }
        Datum::Map(entries) => {
            entries.reverse();
            for (key, value) in entries {
                reordered(key);
                reordered(value);
            }
        }
        Datum::Set(items) => {
            items.reverse();
            for value in items {
                reordered(value);
            }
        }
        Datum::Vector(items) | Datum::List(items) => {
            for value in items {
                reordered(value);
            }
        }
        _ => {}
    }
}

#[test]
fn all_accounting_variants_preserve_complete_identity_and_semantic_field_order() {
    let mut identities = BTreeSet::new();
    for mask in 0..8 {
        let spec = specimen(mask);
        let original = spec.canonical_datum();
        assert_eq!(CommandSpec::from_datum(&original).unwrap(), spec);
        let mut value = original.clone();
        reordered(&mut value);
        assert_eq!(CommandSpec::from_datum(&value).unwrap(), spec);
        identities.insert(spec.id().to_string());
        *field_mut(&mut value, "replay") =
            Datum::Symbol(Symbol::qualified("operation", "exactly-once"));
        let different = CommandSpec::from_datum(&value).unwrap();
        assert_ne!(different.id(), spec.id());
        assert_eq!(different.replay(), CommandReplayPolicy::ExactlyOnce);
    }
    assert_eq!(identities.len(), 8);
}

#[test]
fn full_host_command_round_trips_environment_outputs_cleanup_and_invocation() {
    let output_id = ContentId::from_bytes(Symbol::qualified("test", "content-algorithm"), [7; 32]);
    for interpreter in [false, true] {
        let command = CommandSpec::new(
            ProgramRef::new("installed-program").unwrap(),
            ProjectRootRef::new("work").unwrap(),
            if interpreter {
                CommandInvocation::Interpreter {
                    flags: vec![ArgAtom::new("-c").unwrap()],
                    script: b"printf 'literal script'".to_vec(),
                }
            } else {
                CommandInvocation::Argv(vec![
                    ArgAtom::new("b").unwrap(),
                    ArgAtom::new("a").unwrap(),
                ])
            },
            SealedBindings::try_from_entries([
                ("LITERAL".into(), BindingValue::Literal("verbatim".into())),
                (
                    "ROOT".into(),
                    BindingValue::ProjectRoot(ProjectRootRef::new("work").unwrap()),
                ),
                (
                    "ARTIFACT".into(),
                    BindingValue::PrivateArtifact(
                        PrivateArtifactRef::new("owned-artifact").unwrap(),
                    ),
                ),
            ])
            .unwrap(),
            vec![CommandResource {
                source: "out".into(),
                guest_path: "/out".into(),
                access: ResourceAccess::Writable,
            }],
            ProcessBudget {
                timeout_ms: 1000,
                max_output_bytes: 4096,
                stdin: None,
            },
            OutputContract::new(
                [-1, 0, 7],
                vec![
                    OutputExpectation {
                        resource: "out".into(),
                        relative_path: "exists".into(),
                        state: OutputState::Exists,
                    },
                    OutputExpectation {
                        resource: "out".into(),
                        relative_path: "absent".into(),
                        state: OutputState::Absent,
                    },
                    OutputExpectation {
                        resource: "out".into(),
                        relative_path: "bytes".into(),
                        state: OutputState::FileContent(output_id.clone()),
                    },
                ],
            )
            .unwrap(),
            CleanupContract::process_group(["out".into()]).unwrap(),
            NetworkAccess::Scoped(CapabilityName::new("network/exact-scope")),
            CommandRoute::Process,
            CommandReplayPolicy::ExactlyOnce,
        )
        .unwrap();
        let mut datum = command.canonical_datum();
        for path in [
            vec!["invocation"],
            vec!["environment", "bindings", "LITERAL"],
            vec!["environment", "bindings", "ROOT"],
            vec!["environment", "bindings", "ARTIFACT"],
            vec!["resources", "0"],
            vec!["outputs", "outputs", "0"],
            vec!["outputs", "outputs", "1"],
            vec!["outputs", "outputs", "2"],
            vec!["outputs", "outputs", "2", "state"],
            vec!["network"],
        ] {
            assert_schema_refusal(&datum, &path);
        }
        let mut duplicate = datum.clone();
        let Datum::Set(items) = selected(&mut duplicate, &["cleanup", "scratch-resources"]) else {
            panic!("set")
        };
        items.push(items[0].clone());
        assert!(CommandSpec::from_datum(&duplicate).is_err());
        let mut duplicate = datum.clone();
        let Datum::Map(items) = selected(&mut duplicate, &["environment", "bindings"]) else {
            panic!("map")
        };
        items.push(items[0].clone());
        assert!(CommandSpec::from_datum(&duplicate).is_err());
        let mut changed = datum.clone();
        let Datum::Vector(items) = selected(&mut changed, &["outputs", "outputs"]) else {
            panic!("vector")
        };
        items.reverse();
        assert_ne!(
            CommandSpec::from_datum(&changed).unwrap().id(),
            command.id()
        );
        let mut malformed = datum.clone();
        *selected(
            &mut malformed,
            &["outputs", "outputs", "2", "state", "digest"],
        ) = Datum::Bytes(vec![7; 31]);
        assert!(CommandSpec::from_datum(&malformed).is_err());
        reordered(&mut datum);
        assert_eq!(CommandSpec::from_datum(&datum).unwrap(), command);
        *field_mut(field_mut(&mut datum, "budget"), "stdin") = Datum::Bytes(vec![]);
        if interpreter {
            assert!(CommandSpec::from_datum(&datum).is_err());
        } else {
            let with_stdin = CommandSpec::from_datum(&datum).unwrap();
            assert_ne!(with_stdin.id(), command.id());
            assert_eq!(with_stdin.budget().stdin, Some(vec![]));
        }
    }
}

#[test]
fn malformed_top_level_and_nested_fields_refuse() {
    for path in [
        vec![],
        vec!["budget"],
        vec!["route"],
        vec!["route", "limits"],
        vec!["route", "limits", "cpu"],
        vec!["route", "limits", "memory"],
        vec!["route", "limits", "filesystem"],
        vec!["cleanup"],
        vec!["outputs"],
        vec!["environment"],
        vec!["invocation"],
        vec!["resources", "0"],
        vec!["route", "mounts", "0"],
    ] {
        assert_schema_refusal(&specimen(7).canonical_datum(), &path);
    }
}

fn selected<'a>(value: &'a mut Datum, path: &[&str]) -> &'a mut Datum {
    let Some((first, rest)) = path.split_first() else {
        return value;
    };
    let child = match value {
        Datum::Vector(items) => &mut items[first.parse::<usize>().unwrap()],
        Datum::Map(items) => {
            &mut items
                .iter_mut()
                .find(|(key, _)| key == &Datum::String((*first).into()))
                .unwrap()
                .1
        }
        _ => field_mut(value, first),
    };
    selected(child, rest)
}

fn assert_schema_refusal(original: &Datum, path: &[&str]) {
    for change in 0..5 {
        let mut datum = original.clone();
        let Datum::Node { fields, tag } = selected(&mut datum, path) else {
            panic!("node")
        };
        match change {
            0 => fields.push((Symbol::new("unknown"), Datum::Nil)),
            1 => {
                fields.pop();
            }
            2 => fields.push(fields[0].clone()),
            3 => fields[0].0 = Symbol::qualified("foreign", fields[0].0.name.as_ref()),
            _ => *tag = Symbol::qualified("foreign", tag.name.as_ref()),
        }
        assert!(
            CommandSpec::from_datum(&datum).is_err(),
            "path={path:?} change={change}"
        );
    }
}

#[test]
fn ordered_containers_signed_width_and_equal_cpu_ratios_preserve_semantics() {
    for path in [
        vec!["resources"],
        vec!["route", "mounts"],
        vec!["invocation", "argv"],
    ] {
        for as_set in [false, true] {
            let mut datum = specimen(7).canonical_datum();
            let target = selected(&mut datum, &path);
            let Datum::Vector(items) = target else {
                panic!("vector")
            };
            *target = if as_set {
                Datum::Set(items.clone())
            } else {
                Datum::List(items.clone())
            };
            assert!(CommandSpec::from_datum(&datum).is_err());
        }
    }
    for (text, accepted) in [
        ("-2147483648", true),
        ("2147483647", true),
        ("-2147483649", false),
        ("2147483648", false),
        ("+1", false),
        ("-0", false),
    ] {
        let mut datum = specimen(7).canonical_datum();
        let Datum::Set(codes) = selected(&mut datum, &["outputs", "exit-codes"]) else {
            panic!("set")
        };
        let Datum::Number(code) = &mut codes[0] else {
            panic!("number")
        };
        code.canonical = text.into();
        assert_eq!(CommandSpec::from_datum(&datum).is_ok(), accepted, "{text}");
    }
    let spec = specimen(7);
    let mut datum = spec.canonical_datum();
    for (name, text) in [("quota-us", "400000"), ("period-us", "200000")] {
        let Datum::Number(number) = selected(&mut datum, &["route", "limits", "cpu", name]) else {
            panic!("number")
        };
        number.canonical = text.into();
    }
    assert_ne!(CommandSpec::from_datum(&datum).unwrap().id(), spec.id());
    let mut datum = spec.canonical_datum();
    *selected(&mut datum, &["invocation", "argv"]) = Datum::Vector(vec![
        Datum::String("first".into()),
        Datum::String("second".into()),
    ]);
    let first = CommandSpec::from_datum(&datum).unwrap();
    let Datum::Vector(argv) = selected(&mut datum, &["invocation", "argv"]) else {
        panic!("vector")
    };
    argv.reverse();
    assert_ne!(CommandSpec::from_datum(&datum).unwrap().id(), first.id());
}

#[test]
fn numeric_domains_spellings_width_and_required_nonzero_refuse() {
    for spelling in ["+1", "01", "-1", "-0", "18446744073709551616", "0"] {
        let mut datum = specimen(7).canonical_datum();
        let Datum::Number(number) = field_mut(field_mut(&mut datum, "budget"), "timeout-ms") else {
            panic!("number")
        };
        number.canonical = spelling.into();
        assert!(CommandSpec::from_datum(&datum).is_err());
    }
    let mut datum = specimen(7).canonical_datum();
    let Datum::Number(number) = field_mut(field_mut(&mut datum, "budget"), "timeout-ms") else {
        panic!("number")
    };
    number.domain = Symbol::qualified("numbers", "i64");
    assert!(CommandSpec::from_datum(&datum).is_err());
}

#[test]
fn repeated_set_members_and_duplicate_or_missing_controls_refuse() {
    let mut datum = specimen(7).canonical_datum();
    let Datum::Set(codes) = field_mut(field_mut(&mut datum, "outputs"), "exit-codes") else {
        panic!("set")
    };
    codes.push(codes[0].clone());
    assert!(CommandSpec::from_datum(&datum).is_err());
    for duplicate in [false, true] {
        let mut datum = specimen(7).canonical_datum();
        let Datum::Map(controls) = field_mut(field_mut(&mut datum, "route"), "requirements") else {
            panic!("map")
        };
        if duplicate {
            controls.push(controls[0].clone());
        } else {
            controls.pop();
        }
        assert!(CommandSpec::from_datum(&datum).is_err());
    }
}

#[test]
fn output_contract_has_one_strict_owner_decoder_for_observation_evidence() {
    let command = specimen(7);
    let original = command.outputs().canonical_datum();
    assert_eq!(
        OutputContract::from_datum(&original).unwrap(),
        *command.outputs()
    );

    let mut reordered_datum = original.clone();
    reordered(&mut reordered_datum);
    assert_eq!(
        OutputContract::from_datum(&reordered_datum).unwrap(),
        *command.outputs()
    );

    let mut malformed = original;
    *field_mut(&mut malformed, "outputs") = Datum::List(Vec::new());
    assert!(OutputContract::from_datum(&malformed).is_err());
}
