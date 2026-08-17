use sim_kernel::Value;
use sim_lib_machine::{ManagedRootSource, ValueWidthPolicy};
use sim_lib_mutation::{ManagedHandle, ManagedId};

/// JVM primitive computational categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimitiveCategory {
    /// `boolean`, `byte`, `char`, `short`, and `int` computational values.
    Int,
    /// IEEE-754 binary32 computational values.
    Float,
    /// Signed 64-bit integer computational values.
    Long,
    /// IEEE-754 binary64 computational values.
    Double,
}

impl PrimitiveCategory {
    /// Returns the JVM logical slot width, independently of host representation.
    pub const fn logical_width(self) -> usize {
        match self {
            Self::Int | Self::Float => 1,
            Self::Long | Self::Double => 2,
        }
    }
}

/// A nullable, non-rooting handle into the shared managed arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JvmReference(Option<ManagedHandle>);

impl JvmReference {
    /// The JVM null reference.
    pub const NULL: Self = Self(None);

    /// Wraps a shared managed handle.
    pub const fn managed(handle: ManagedHandle) -> Self {
        Self(Some(handle))
    }

    /// Returns the managed handle, or `None` for null.
    pub const fn handle(self) -> Option<ManagedHandle> {
        self.0
    }
}

impl ManagedRootSource for JvmReference {
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        self.0.is_none_or(|handle| visit(handle.id()))
    }
}

/// Values carried by the JVM profile on the neutral machine storage organ.
#[derive(Clone, Debug)]
pub enum JvmValue {
    /// A category-1 signed integer computational value.
    Int(i32),
    /// A category-1 float represented by its exact IEEE bits.
    Float(u32),
    /// A category-2 signed integer computational value.
    Long(i64),
    /// A category-2 double represented by its exact IEEE bits.
    Double(u64),
    /// A category-1 managed reference.
    Reference(JvmReference),
    /// A category-1 universal SIM value crossing an explicit host boundary.
    Kernel(Value),
}

impl JvmValue {
    /// Returns the logical width charged by shared machine storage.
    pub const fn logical_width(&self) -> usize {
        match self {
            Self::Long(_) | Self::Double(_) => 2,
            Self::Int(_) | Self::Float(_) | Self::Reference(_) | Self::Kernel(_) => 1,
        }
    }
}

impl ManagedRootSource for JvmValue {
    fn visit_managed_roots(&self, visit: &mut dyn FnMut(ManagedId) -> bool) -> bool {
        match self {
            Self::Reference(reference) => reference.visit_managed_roots(visit),
            Self::Int(_) | Self::Float(_) | Self::Long(_) | Self::Double(_) | Self::Kernel(_) => {
                true
            }
        }
    }
}

/// Width policy connecting JVM values to shared machine storage.
pub struct JvmValueWidth;

impl ValueWidthPolicy for JvmValueWidth {
    type Value = JvmValue;

    fn width(value: &Self::Value) -> usize {
        value.logical_width()
    }
}

/// JVM method return categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnCategory {
    /// No value is returned.
    Void,
    /// A primitive value is returned.
    Primitive(PrimitiveCategory),
    /// A managed reference is returned.
    Reference,
}

impl ReturnCategory {
    /// Returns the logical width transferred by a return.
    pub const fn logical_width(self) -> usize {
        match self {
            Self::Void => 0,
            Self::Primitive(category) => category.logical_width(),
            Self::Reference => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use sim_lib_control::WorkLimit;
    use sim_lib_machine::RootSnapshot;
    use sim_lib_mutation::{HardCappedRetainPolicy, ManagedArena, ManagedNode};

    use super::*;

    #[test]
    fn machine_snapshot_sees_every_jvm_reference_and_skips_primitives_and_null() {
        let mut arena = ManagedArena::new(HardCappedRetainPolicy::new(3).unwrap());
        let handles = (0..3)
            .map(|_| arena.allocate(ManagedNode::new(())).unwrap())
            .collect::<Vec<_>>();
        let values = vec![
            JvmValue::Reference(JvmReference::managed(handles[0])),
            JvmValue::Int(7),
            JvmValue::Reference(JvmReference::NULL),
            JvmValue::Reference(JvmReference::managed(handles[1])),
            JvmValue::Long(9),
            JvmValue::Reference(JvmReference::managed(handles[2])),
        ];

        assert_eq!(
            RootSnapshot::scan(&values, WorkLimit(3)).unwrap().roots(),
            handles.iter().map(|handle| handle.id()).collect::<Vec<_>>()
        );
        assert!(RootSnapshot::scan(&values, WorkLimit(2)).is_err());
    }
}
