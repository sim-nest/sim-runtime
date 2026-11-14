// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use std::process::{Command, Stdio};
use std::time::Instant;

use sim_kernel::{Datum, Symbol};

use crate::command::{self, WorldCommandLib};

const SEAL_SAMPLES: usize = 5;
const SEAL_MEASURE_CEILING_MS: u128 = 10_614;
const SAMPLE_TEST: &str =
    "seal_compile_tests::seal_measure_five_isolated_samples_stay_inside_the_ceiling";
const CHILD_ENV: &str = "SIM_SEAL_MEASURE_CHILD";
const ADMITTED_TRACE: &str = "compile checked seal same lock moved missing missing-input corrupt corrupt-input target wrong-target stub stub-only resident true effects 0 subprocesses 1";

fn within_seal_ceiling(elapsed_ms: u128) -> bool {
    elapsed_ms <= SEAL_MEASURE_CEILING_MS
}

#[test]
fn seal_measure_ceiling_admits_bound_and_refuses_next() {
    assert!(within_seal_ceiling(SEAL_MEASURE_CEILING_MS));
    assert!(!within_seal_ceiling(SEAL_MEASURE_CEILING_MS + 1));
    assert_eq!(SEAL_MEASURE_CEILING_MS, 10_614);
    assert_eq!(SEAL_MEASURE_CEILING_MS + 1, 10_615);
}

#[test]
fn the_loaded_measure_command_checks_one_admitted_input() {
    let command = WorldCommandLib::new().expect("command");
    let argv = ["sim".to_owned(), "measure".to_owned(), "seal".to_owned()];
    let first = command.run(&argv).expect("measure");
    assert_eq!(admitted_trace(&first), ADMITTED_TRACE);
    let second = command.run(&argv).expect("cached");
    assert_eq!(command::render(&first), command::render(&second));
}

/// Five isolated runs of the loaded measure seal command.
///
/// The ceiling is ceil(max(sample) * 1.15) milliseconds. 10614 admits and
/// 10615 refuses. The command keeps its record for the life of one process,
/// so each sample is a separate process. The default suite skips these samples.
/// Run them with
/// `cargo test -p sim-lib-world --lib seal_measure_five_isolated -- --ignored`.
#[test]
#[ignore = "five isolated loaded measure seal samples"]
fn seal_measure_five_isolated_samples_stay_inside_the_ceiling() {
    if std::env::var_os(CHILD_ENV).is_some() {
        one_loaded_sample();
        return;
    }
    let mut raw_ms = Vec::with_capacity(SEAL_SAMPLES);
    for _ in 0..SEAL_SAMPLES {
        raw_ms.push(run_isolated_sample());
    }
    assert_eq!(raw_ms.len(), SEAL_SAMPLES);
    let max_ms = raw_ms.iter().copied().max().unwrap_or(0);
    let within = raw_ms.iter().all(|sample| within_seal_ceiling(*sample));
    let raw = raw_ms
        .iter()
        .map(u128::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let derived_ms = max_ms.saturating_mul(115).saturating_add(99) / 100;
    println!(
        "\nseal-measure samples {SEAL_SAMPLES} ceiling-ms {SEAL_MEASURE_CEILING_MS} admits-ms {SEAL_MEASURE_CEILING_MS} refuses-ms {} derived-ms {derived_ms} max-ms {max_ms} within {within} raw-ms {raw} {ADMITTED_TRACE}",
        SEAL_MEASURE_CEILING_MS + 1
    );
}

fn one_loaded_sample() {
    let command = WorldCommandLib::new().expect("command");
    let argv = ["sim".to_owned(), "measure".to_owned(), "seal".to_owned()];
    let started = Instant::now();
    let value = command.run(&argv).expect("measure seal");
    let rendered = command::render(&value);
    let elapsed_ms = started.elapsed().as_millis();
    let trace = admitted_trace(&value);
    assert_eq!(trace, ADMITTED_TRACE);
    assert!(
        rendered.contains("seal-measure-v1"),
        "rendered record {rendered}"
    );
    assert!(
        within_seal_ceiling(elapsed_ms),
        "sample {elapsed_ms} exceeds {SEAL_MEASURE_CEILING_MS}"
    );
    println!("\nseal-sample ms {elapsed_ms} {trace}");
}

fn run_isolated_sample() -> u128 {
    let exe = std::env::current_exe().expect("test executable");
    let output = Command::new(exe)
        .args([
            "--exact",
            SAMPLE_TEST,
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_ENV, "1")
        .stdin(Stdio::null())
        .output()
        .expect("isolated sample");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "isolated sample failed\n{stdout}\n{stderr}"
    );
    assert!(
        stdout.contains("test result: ok. 1 passed;"),
        "isolated sample did not run one test\n{stdout}\n{stderr}"
    );
    sample_millis(&stdout)
}

fn sample_millis(stdout: &str) -> u128 {
    let mut found = None;
    for line in stdout.lines() {
        let Some(index) = line.find("seal-sample ms ") else {
            continue;
        };
        let rest = &line[index + "seal-sample ms ".len()..];
        let (millis, trace) = rest
            .split_once(' ')
            .unwrap_or_else(|| panic!("sample line {line}"));
        let elapsed_ms: u128 = millis
            .parse()
            .unwrap_or_else(|_| panic!("sample millis {line}"));
        assert_eq!(trace, ADMITTED_TRACE, "{line}");
        assert!(found.is_none(), "two samples in one process\n{stdout}");
        found = Some(elapsed_ms);
    }
    found.unwrap_or_else(|| panic!("missing seal-sample line\n{stdout}"))
}

fn admitted_trace(value: &Datum) -> String {
    let fields = node(value, "world/seal-measure-v1");
    format!(
        "compile {} seal {} lock {} missing {} corrupt {} target {} stub {} resident {} effects {} subprocesses {}",
        text(&fields, "compile"),
        text(&fields, "seal"),
        text(&fields, "lock"),
        text(&fields, "missing"),
        text(&fields, "corrupt"),
        text(&fields, "target"),
        text(&fields, "stub"),
        bool_field(&fields, "resident"),
        number(&fields, "effects"),
        number(&fields, "subprocesses"),
    )
}

fn node(value: &Datum, tag: &str) -> Vec<(Symbol, Datum)> {
    let Datum::Node { tag: found, fields } = value else {
        panic!("record {value:?}");
    };
    assert_eq!(found.as_qualified_str(), tag, "{value:?}");
    fields.clone()
}

fn text(fields: &[(Symbol, Datum)], name: &str) -> String {
    let Some(Datum::String(text)) = field(fields, name) else {
        panic!("missing {name}");
    };
    text.clone()
}

fn number(fields: &[(Symbol, Datum)], name: &str) -> u64 {
    let Some(Datum::Number(number)) = field(fields, name) else {
        panic!("missing {name}");
    };
    number
        .canonical
        .parse()
        .unwrap_or_else(|_| panic!("{name}"))
}

fn bool_field(fields: &[(Symbol, Datum)], name: &str) -> bool {
    matches!(field(fields, name), Some(Datum::Bool(true)))
}

fn field<'a>(fields: &'a [(Symbol, Datum)], name: &str) -> Option<&'a Datum> {
    fields
        .iter()
        .find(|(field, _)| field.to_string() == name)
        .map(|(_, value)| value)
}
