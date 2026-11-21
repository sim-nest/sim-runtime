// SPDX-License-Identifier: MPL-2.0
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Loaded control, cleanup, and stop records for one subject.
//!
//! The control case is the durable control channel. The liveness case and
//! the cleanup measurement close modeled resources. The stop measurement
//! joins the worker that does not return before its record exists, and its
//! OS flag stays clear. Cleanup closure does not set that flag. This module
//! does not derive a completion seal.

use sha2::{Digest, Sha256};

use crate::WorldError;

const CONTROL: &str = "queued 8 stopped 0 log stop os 0";
const LIVENESS: &str = "stalled s0 since 0 failures 0 work 1 idle 1 beats 7 closed 1 os 0 wake resource-offer,control,fact-refresh";
const CLOSED_LINE: &str = "closed 0 23 stopped 0 1 0";
const ZERO_SEAL: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// Records the loaded schedule, cleanup, and stop commands return.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FinalSchedule {
    /// Detail of the loaded control case.
    pub control: String,
    /// Detail of the loaded liveness case.
    pub liveness: String,
    /// Closed modeled-resource line from the loaded cleanup trace.
    pub cleanup_closed: String,
    /// Hung-worker count from the loaded stop record.
    pub stop_hung: String,
    /// OS-termination flag from the loaded stop record.
    pub stop_os_terminated: String,
    /// Reopen flag from the loaded stop record.
    pub stop_reopened: String,
    /// Digest of the control detail.
    pub control_seal: String,
    /// Digest of the liveness detail.
    pub liveness_seal: String,
    /// Digest of the cleanup trace.
    pub cleanup_seal: String,
    /// Digest of the stop trace.
    pub stop_seal: String,
}

/// Runs the loaded control, liveness, cleanup, and stop commands.
///
/// # Errors
///
/// Returns an error when a command cannot run, a record differs from the
/// one that command exists to show, or two seals collide.
pub fn execute() -> Result<FinalSchedule, WorldError> {
    let command = crate::command::WorldCommandLib::new()?;
    let (control, control_seal) = sealed_schedule(&command, "control", CONTROL, "9")?;
    let (liveness, liveness_seal) = sealed_schedule(&command, "liveness", LIVENESS, "10")?;
    let (cleanup_closed, cleanup_seal) = sealed_cleanup(&command)?;
    let (stop_hung, stop_os_terminated, stop_reopened, stop_seal) = sealed_stop(&command)?;
    let seals = [
        control_seal.as_str(),
        liveness_seal.as_str(),
        cleanup_seal.as_str(),
        stop_seal.as_str(),
    ];
    let mut unique = std::collections::BTreeSet::new();
    if seals
        .iter()
        .any(|seal| !hex_seal(seal) || !unique.insert(*seal))
    {
        return Err(fault("seal-identity"));
    }
    Ok(FinalSchedule {
        control,
        liveness,
        cleanup_closed,
        stop_hung,
        stop_os_terminated,
        stop_reopened,
        control_seal,
        liveness_seal,
        cleanup_seal,
        stop_seal,
    })
}

fn sealed_schedule(
    command: &crate::command::WorldCommandLib,
    case: &str,
    detail: &str,
    invokes: &str,
) -> Result<(String, String), WorldError> {
    let datum = command.run(&argv("schedule", case))?;
    refuse_copied(&datum)?;
    if kind_of(&datum) != "world/schedule-v1" {
        return Err(fault("schedule-record"));
    }
    require(&text(&datum, "case")?, case, "case")?;
    require(&text(&datum, "detail")?, detail, "detail")?;
    require(&number(&datum, "effects")?, "0", "effects")?;
    require(&number(&datum, "invokes")?, invokes, "invokes")?;
    require(&number(&datum, "subprocesses")?, "0", "subprocesses")?;
    if text_bool(&datum, "resident")? != "true" {
        return Err(fault("resident"));
    }
    Ok((
        detail.to_owned(),
        seal_of(&format!("schedule {case} {detail}")),
    ))
}

fn sealed_cleanup(
    command: &crate::command::WorldCommandLib,
) -> Result<(String, String), WorldError> {
    let datum = command.run(&argv("measure", "cleanup"))?;
    refuse_copied(&datum)?;
    if kind_of(&datum) != "world/cleanup-beats-v1" {
        return Err(fault("cleanup-record"));
    }
    require(&text(&datum, "command")?, "cleanup", "command")?;
    require(&number(&datum, "effects")?, "0", "effects")?;
    require(&number(&datum, "subprocesses")?, "0", "subprocesses")?;
    if text_bool(&datum, "os-terminated")? != "false" || text_bool(&datum, "reopened")? != "true" {
        return Err(fault("cleanup-record"));
    }
    let trace = text(&datum, "trace")?;
    if !accept_cleanup(&trace) {
        return Err(fault("cleanup-trace"));
    }
    Ok((CLOSED_LINE.to_owned(), seal_of(&format!("cleanup {trace}"))))
}

fn sealed_stop(
    command: &crate::command::WorldCommandLib,
) -> Result<(String, String, String, String), WorldError> {
    let datum = command.run(&argv("measure", "stop"))?;
    refuse_copied(&datum)?;
    if kind_of(&datum) != "world/stop-ack-v1" {
        return Err(fault("stop-record"));
    }
    require(&text(&datum, "command")?, "stop", "command")?;
    require(&number(&datum, "hung")?, "1", "hung")?;
    require(&number(&datum, "cases")?, "100", "cases")?;
    require(&number(&datum, "effects")?, "0", "effects")?;
    require(&number(&datum, "subprocesses")?, "0", "subprocesses")?;
    let os_terminated = text_bool(&datum, "os-terminated")?;
    let reopened = text_bool(&datum, "reopened")?;
    if os_terminated != "false" || reopened != "true" || text_bool(&datum, "resident")? != "true" {
        return Err(fault("stop-record"));
    }
    let trace = text(&datum, "trace")?;
    if !accept_stop(&trace) {
        return Err(fault("stop-trace"));
    }
    Ok((
        "1".to_owned(),
        os_terminated,
        reopened,
        seal_of(&format!("stop {trace}")),
    ))
}

fn accept_cleanup(trace: &str) -> bool {
    let mut closed = false;
    let mut rows = 0;
    for line in trace.lines() {
        if line.split_whitespace().nth(6) != Some("0") {
            return false;
        }
        if line == CLOSED_LINE {
            closed = true;
        }
        rows += 1;
    }
    closed && rows > 1 && trace.ends_with('\n')
}

fn accept_stop(trace: &str) -> bool {
    let mut rows = 0;
    let mut hung = 0;
    for line in trace.lines() {
        let mut parts = line.split(' ');
        let _index = parts.next();
        let kind = parts.next();
        let verb = parts.next();
        let _at = parts.next();
        let mode = parts.next();
        let queue = parts.next();
        let os = parts.next();
        if parts.next().is_some()
            || verb != Some("stop")
            || mode != Some("stopped")
            || queue != Some("0")
            || os != Some("0")
        {
            return false;
        }
        match kind {
            Some("hung") if rows == 0 => hung += 1,
            Some("blocked") if rows > 0 => {}
            _ => return false,
        }
        rows += 1;
    }
    rows == 100 && hung == 1 && trace.ends_with('\n')
}

fn argv(verb: &str, case: &str) -> Vec<String> {
    vec!["sim".to_owned(), verb.to_owned(), case.to_owned()]
}

fn seal_of(report: &str) -> String {
    hex(Sha256::digest(report.as_bytes()).as_slice())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    out
}

fn hex_seal(value: &str) -> bool {
    value.len() == 64
        && value != ZERO_SEAL
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn require(found: &str, intended: &str, label: &str) -> Result<(), WorldError> {
    if found == intended {
        Ok(())
    } else {
        Err(fault(label))
    }
}

fn kind_of(value: &sim_kernel::Datum) -> String {
    match value {
        sim_kernel::Datum::Node { tag, .. } => tag.as_qualified_str(),
        _ => "missing".to_owned(),
    }
}

fn text(value: &sim_kernel::Datum, name: &str) -> Result<String, WorldError> {
    match field(value, name) {
        Some(sim_kernel::Datum::String(text)) => Ok(text.clone()),
        _ => Err(fault(name)),
    }
}

fn number(value: &sim_kernel::Datum, name: &str) -> Result<String, WorldError> {
    match field(value, name) {
        Some(sim_kernel::Datum::Number(number)) if !number.canonical.is_empty() => {
            Ok(number.canonical.clone())
        }
        _ => Err(fault(name)),
    }
}

fn text_bool(value: &sim_kernel::Datum, name: &str) -> Result<String, WorldError> {
    match field(value, name) {
        Some(sim_kernel::Datum::Bool(true)) => Ok("true".to_owned()),
        Some(sim_kernel::Datum::Bool(false)) => Ok("false".to_owned()),
        _ => Err(fault(name)),
    }
}

fn field<'a>(value: &'a sim_kernel::Datum, name: &str) -> Option<&'a sim_kernel::Datum> {
    let sim_kernel::Datum::Node { fields, .. } = value else {
        return None;
    };
    fields
        .iter()
        .find(|(symbol, _)| symbol.to_string() == name)
        .map(|(_, found)| found)
}

fn refuse_copied(value: &sim_kernel::Datum) -> Result<(), WorldError> {
    if copied(value) {
        Err(fault("copied-marker"))
    } else {
        Ok(())
    }
}

fn copied(value: &sim_kernel::Datum) -> bool {
    match value {
        sim_kernel::Datum::String(text) => leaked(text),
        sim_kernel::Datum::Node { fields, .. } => fields.iter().any(|(_, child)| copied(child)),
        sim_kernel::Datum::Vector(items) | sim_kernel::Datum::List(items) => {
            items.iter().any(copied)
        }
        _ => false,
    }
}

fn leaked(value: &str) -> bool {
    value.contains("private-material:")
        || value.contains("completion/claim-")
        || value.contains("/home/")
        || value.contains("bo-private")
        || value.contains("docs/private")
        || value.contains("sim-private/")
}

fn fault(detail: &str) -> WorldError {
    WorldError::Qualification(detail.to_owned())
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_loaded_commands_keep_control_closure_and_the_joined_worker() {
        let loaded = super::execute().expect("schedule");
        assert_eq!(loaded.control, super::CONTROL);
        assert_eq!(loaded.liveness, super::LIVENESS);
        assert_eq!(loaded.cleanup_closed, super::CLOSED_LINE);
        assert_eq!(loaded.stop_hung, "1");
        assert_eq!(loaded.stop_os_terminated, "false");
        assert_eq!(loaded.stop_reopened, "true");
        let seals = [
            loaded.control_seal.as_str(),
            loaded.liveness_seal.as_str(),
            loaded.cleanup_seal.as_str(),
            loaded.stop_seal.as_str(),
        ];
        let mut seen = std::collections::BTreeSet::new();
        for seal in seals {
            assert_ne!(seal, super::ZERO_SEAL);
            assert!(seen.insert(seal), "{seal}");
        }
    }
}
