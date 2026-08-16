//! Closed intrinsic admission table and managed primitive boxes.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use sim_lib_gc_tracing::CollectionLimits;
use sim_lib_mutation::{ArenaError, ManagedHandle, RootedHandle};

use crate::{JvmHeap, JvmRole};

/// A primitive value stored by a Java wrapper object.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum BoxValue {
    /// `java.lang.Boolean` payload.
    Boolean(bool),
    /// `java.lang.Byte` payload.
    Byte(i8),
    /// `java.lang.Character` payload.
    Character(u16),
    /// `java.lang.Short` payload.
    Short(i16),
    /// `java.lang.Integer` payload.
    Integer(i32),
    /// `java.lang.Long` payload.
    Long(i64),
    /// `java.lang.Float` payload, preserving IEEE bits.
    Float(u32),
    /// `java.lang.Double` payload, preserving IEEE bits.
    Double(u64),
}

impl BoxValue {
    fn cached(self) -> bool {
        match self {
            Self::Boolean(_) | Self::Byte(_) => true,
            Self::Character(value) => value <= 127,
            Self::Short(value) => (-128..=127).contains(&value),
            Self::Integer(value) => (-128..=127).contains(&value),
            Self::Long(value) => (-128..=127).contains(&value),
            Self::Float(_) | Self::Double(_) => false,
        }
    }
}

/// A managed primitive wrapper reference and its stable identity hash.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PrimitiveBox {
    handle: ManagedHandle,
    value: BoxValue,
}

impl PrimitiveBox {
    /// Returns the managed object handle.
    pub const fn handle(self) -> ManagedHandle {
        self.handle
    }

    /// Returns the primitive payload.
    pub const fn value(self) -> BoxValue {
        self.value
    }

    /// Returns a stable, non-zero identity hash derived from managed identity.
    pub const fn identity_hash(self) -> i32 {
        let ordinal = self.handle.id().allocation_ordinal();
        let folded = (ordinal as u32) ^ ((ordinal >> 32) as u32);
        let hash = folded.wrapping_add(1) & 0x7fff_ffff;
        if hash == 0 { 1 } else { hash as i32 }
    }
}

/// Managed owner for primitive wrapper values and the JDK-mandated caches.
pub struct PrimitiveBoxes {
    heap: JvmHeap,
    cache: BTreeMap<BoxValue, (PrimitiveBox, RootedHandle)>,
}

impl PrimitiveBoxes {
    /// Creates a bounded managed box owner.
    pub fn new(cap: usize, limits: CollectionLimits) -> Result<Self, ArenaError> {
        Ok(Self {
            heap: JvmHeap::new(cap, limits)?,
            cache: BTreeMap::new(),
        })
    }

    /// Boxes a value, preserving identity exactly for the specified cache ranges.
    pub fn box_value(&mut self, value: BoxValue) -> Result<PrimitiveBox, ArenaError> {
        if value.cached()
            && let Some((boxed, _root)) = self.cache.get(&value)
        {
            return Ok(*boxed);
        }
        let handle = self.heap.allocate(JvmRole::PrimitiveBox)?;
        let boxed = PrimitiveBox { handle, value };
        if value.cached() {
            let root = self.heap.root(handle)?;
            self.cache.insert(value, (boxed, root));
        }
        Ok(boxed)
    }

    /// Returns the number of managed wrapper allocations still live.
    pub fn live_len(&self) -> usize {
        self.heap.live_len()
    }
}

/// Whether an intrinsic table row is executable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IntrinsicSupport {
    /// The row may proceed to intrinsic execution.
    Supported,
    /// The row is a declared gap and must fail during admission.
    Unsupported,
}

/// One exact member in the closed intrinsic table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IntrinsicMember {
    /// Internal class name.
    pub class: &'static str,
    /// JVM member name.
    pub name: &'static str,
    /// Exact JVM descriptor.
    pub descriptor: &'static str,
    /// Argument Shape declaration.
    pub arguments_shape: &'static str,
    /// Result Shape declaration.
    pub result_shape: &'static str,
    /// Required capability, or `none`.
    pub capability: &'static str,
    /// Declared effect class.
    pub effect: &'static str,
    /// Deterministic work charge.
    pub work: u32,
    /// Admission disposition.
    pub support: IntrinsicSupport,
}

macro_rules! wrapper_rows {
    ($(($class:literal, $primitive:literal, $descriptor:literal)),+ $(,)?) => { &[
        $(IntrinsicMember { class: $class, name: "valueOf", descriptor: concat!("(", $descriptor, ")L", $class, ";"), arguments_shape: concat!("tuple<", $primitive, ">"), result_shape: concat!("reference<", $class, ">"), capability: "none", effect: "managed-allocation", work: 2, support: IntrinsicSupport::Supported },
        IntrinsicMember { class: $class, name: concat!($primitive, "Value"), descriptor: concat!("()", $descriptor), arguments_shape: concat!("receiver<", $class, ">"), result_shape: $primitive, capability: "none", effect: "pure", work: 1, support: IntrinsicSupport::Supported },
        IntrinsicMember { class: $class, name: "<init>", descriptor: concat!("(", $descriptor, ")V"), arguments_shape: concat!("receiver<", $class, ">+tuple<", $primitive, ">"), result_shape: "void", capability: "none", effect: "unsupported", work: 0, support: IntrinsicSupport::Unsupported },)+
    ] };
}

/// The single closed table used for intrinsic lookup and admission.
pub const INTRINSIC_TABLE: &[IntrinsicMember] = wrapper_rows![
    ("java/lang/Boolean", "boolean", "Z"),
    ("java/lang/Byte", "byte", "B"),
    ("java/lang/Character", "char", "C"),
    ("java/lang/Short", "short", "S"),
    ("java/lang/Integer", "int", "I"),
    ("java/lang/Long", "long", "J"),
    ("java/lang/Float", "float", "F"),
    ("java/lang/Double", "double", "D"),
];

/// Intrinsic lookup or admission failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IntrinsicError {
    /// The exact member is absent from the closed table.
    Undeclared {
        /// Internal class name supplied by the caller.
        class: String,
        /// Member name supplied by the caller.
        name: String,
        /// Descriptor supplied by the caller.
        descriptor: String,
    },
    /// The member is explicitly declared unsupported.
    Unsupported {
        /// Internal class name from the manifest row.
        class: &'static str,
        /// Member name from the manifest row.
        name: &'static str,
        /// Descriptor from the manifest row.
        descriptor: &'static str,
    },
}

impl fmt::Display for IntrinsicError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Undeclared {
                class,
                name,
                descriptor,
            } => write!(f, "undeclared intrinsic {class}.{name}{descriptor}"),
            Self::Unsupported {
                class,
                name,
                descriptor,
            } => write!(f, "unsupported intrinsic {class}.{name}{descriptor}"),
        }
    }
}
impl Error for IntrinsicError {}

/// Resolves an exact tuple and rejects unsupported rows before execution.
pub fn admit_intrinsic(
    class: &str,
    name: &str,
    descriptor: &str,
) -> Result<&'static IntrinsicMember, IntrinsicError> {
    let Some(member) = INTRINSIC_TABLE
        .iter()
        .find(|row| (row.class, row.name, row.descriptor) == (class, name, descriptor))
    else {
        return Err(IntrinsicError::Undeclared {
            class: class.into(),
            name: name.into(),
            descriptor: descriptor.into(),
        });
    };
    if member.support == IntrinsicSupport::Unsupported {
        return Err(IntrinsicError::Unsupported {
            class: member.class,
            name: member.name,
            descriptor: member.descriptor,
        });
    }
    Ok(member)
}
