use sim_kernel::{CapabilityName, Ref, Symbol};
use sim_lib_operation_gate::{ExecutionMode, OperationDeclaration};

fn main() {
    let warehouse = declaration("warehouse/recount", "inventory/read", ExecutionMode::Recorded);
    let microscope = declaration("microscope/calibrate", "lab/calibrate", ExecutionMode::Reviewed);
    println!("warehouse mode: {:?}", warehouse.mode);
    println!("microscope mode: {:?}", microscope.mode);
    println!("automotive assumptions: 0");
}

fn declaration(operation: &str, capability: &str, mode: ExecutionMode) -> OperationDeclaration {
    OperationDeclaration { operation: operation.into(), subject: Ref::Symbol(Symbol::new(operation)), capability: CapabilityName::new(capability), mode }
}
