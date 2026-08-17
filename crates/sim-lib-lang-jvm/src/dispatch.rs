//! The single dense dispatch boundary for prepared JVM instructions.

use crate::PreparedJvmInstruction;
use crate::verifier::PreparedDispatchFamily;

/// Consumer of the generated prepared-instruction families.
///
/// The driver selects one of these methods using identity frozen by preparation. Implementations
/// execute already-lowered operands; they must never parse classfile bytes or consult a manifest.
pub trait PreparedDispatch {
    /// Result of one instruction dispatch.
    type Output;

    /// Executes a constants, locals, or stack instruction.
    fn storage(&mut self, instruction: &PreparedJvmInstruction) -> Self::Output;
    /// Executes an arithmetic, comparison, or conversion instruction.
    fn numeric(&mut self, instruction: &PreparedJvmInstruction) -> Self::Output;
    /// Executes a branch, switch, or return instruction.
    fn control(&mut self, instruction: &PreparedJvmInstruction) -> Self::Output;
    /// Executes an object, array, field, invocation, or allocation instruction.
    fn object(&mut self, instruction: &PreparedJvmInstruction) -> Self::Output;
}

/// Dispatches one prepared instruction through its generated dense family identity.
pub fn dispatch_prepared<D: PreparedDispatch>(
    instruction: &PreparedJvmInstruction,
    driver: &mut D,
) -> D::Output {
    match instruction.dispatch_family() {
        PreparedDispatchFamily::Storage => driver.storage(instruction),
        PreparedDispatchFamily::Numeric => driver.numeric(instruction),
        PreparedDispatchFamily::Control => driver.control(instruction),
        PreparedDispatchFamily::Object => driver.object(instruction),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[test]
    fn drive_boundary_cannot_reach_classfile_decode_or_manifest_lookup() {
        let source =
            fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/dispatch.rs")).unwrap();
        assert!(!source.contains(concat!("decode_", "instructions(")));
        assert!(!source.contains(concat!(".meta", "data()")));
        assert!(!source.contains(concat!("OP", "CODES")));
    }
}
