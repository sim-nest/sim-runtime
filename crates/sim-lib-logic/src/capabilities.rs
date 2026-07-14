//! Capability names used by the logic library.

use sim_kernel::CapabilityName;

/// The capability gating logic-database writes (`logic.db.write`).
pub fn logic_db_write_capability() -> CapabilityName {
    CapabilityName::new("logic.db.write")
}

/// The capability gating logic file consulting (`logic.consult.file`).
pub fn logic_consult_file_capability() -> CapabilityName {
    CapabilityName::new("logic.consult.file")
}

/// The capability gating logic tool calls (`logic.tool-call`).
pub fn logic_tool_call_capability() -> CapabilityName {
    CapabilityName::new("logic.tool-call")
}

#[cfg(test)]
mod tests {
    use super::{
        logic_consult_file_capability, logic_db_write_capability, logic_tool_call_capability,
    };

    #[test]
    fn capability_tokens_are_stable() {
        assert_eq!(logic_db_write_capability().as_str(), "logic.db.write");
        assert_eq!(
            logic_consult_file_capability().as_str(),
            "logic.consult.file"
        );
        assert_eq!(logic_tool_call_capability().as_str(), "logic.tool-call");
    }
}
