use sim_lib_control::WorkLimit;
use sim_lib_lang_jvm::{
    FailureCondition, FailureHome, JvmValue, JvmValueWidth, PrimitiveCategory, ReturnCategory,
};
use sim_lib_machine::UnitStack;

#[test]
fn category_two_uses_two_machine_units_without_jvm_slot_storage() {
    let mut operands = UnitStack::<JvmValueWidth>::new(WorkLimit(4));
    operands.push(JvmValue::Int(1)).unwrap();
    assert_eq!(operands.depth(), 1);
    operands.push(JvmValue::Long(2)).unwrap();
    assert_eq!(operands.depth(), 3);
    assert_eq!(PrimitiveCategory::Double.logical_width(), 2);
    assert_eq!(
        ReturnCategory::Primitive(PrimitiveCategory::Long).logical_width(),
        2
    );
}

#[test]
fn boundary_table_exhaustively_assigns_each_failure_once() {
    fn asserted_by_exhaustive_match(condition: FailureCondition) -> FailureHome {
        match condition {
            FailureCondition::NullDereference => FailureHome::JavaThrowable,
            FailureCondition::Arithmetic => FailureHome::JavaThrowable,
            FailureCondition::ClassCast => FailureHome::JavaThrowable,
            FailureCondition::InvalidClassfile => FailureHome::Admission,
            FailureCondition::UnauthorizedLinkage => FailureHome::Admission,
            FailureCondition::ExecutionAdmissionLimit => FailureHome::Admission,
            FailureCondition::OperandCapacity => FailureHome::Resource,
            FailureCondition::FrameCapacity => FailureHome::Resource,
            FailureCondition::WorkBudget => FailureHome::Resource,
            FailureCondition::ManagedObjectBudget => FailureHome::Resource,
            FailureCondition::ClassfileByteBudget => FailureHome::Resource,
            FailureCondition::InternedStringBudget => FailureHome::Resource,
        }
    }

    let all = [
        FailureCondition::NullDereference,
        FailureCondition::Arithmetic,
        FailureCondition::ClassCast,
        FailureCondition::InvalidClassfile,
        FailureCondition::UnauthorizedLinkage,
        FailureCondition::ExecutionAdmissionLimit,
        FailureCondition::OperandCapacity,
        FailureCondition::FrameCapacity,
        FailureCondition::WorkBudget,
        FailureCondition::ManagedObjectBudget,
        FailureCondition::ClassfileByteBudget,
        FailureCondition::InternedStringBudget,
    ];
    for condition in all {
        assert_eq!(condition.home(), asserted_by_exhaustive_match(condition));
    }
}
