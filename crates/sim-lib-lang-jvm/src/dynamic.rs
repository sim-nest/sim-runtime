//! Closed, lazy linkage for the admitted JVM dynamic string-concatenation protocol.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use sim_lib_mutation::ManagedHandle;
use sim_text::CodeUnitString;

use crate::{
    ClassDefinition, ClassDefinitionId, ClassSpaceRevision, JavaString, JvmEdge, JvmGraphError,
    JvmHeap, JvmReference, JvmRole, JvmValue,
};

/// The sole bootstrap method admitted by this JVM profile.
pub const STRING_CONCAT_BOOTSTRAP_OWNER: &str = "java/lang/invoke/StringConcatFactory";
/// Name of the sole admitted bootstrap method.
pub const STRING_CONCAT_BOOTSTRAP_NAME: &str = "makeConcatWithConstants";
/// Exact descriptor of the sole admitted bootstrap method.
pub const STRING_CONCAT_BOOTSTRAP_DESCRIPTOR: &str = "(Ljava/lang/invoke/MethodHandles$Lookup;Ljava/lang/String;Ljava/lang/invoke/MethodType;Ljava/lang/String;[Ljava/lang/Object;)Ljava/lang/invoke/CallSite;";

/// A bootstrap method reference, retained verbatim for fail-closed diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DynamicBootstrap {
    /// Internal JVM class name of the bootstrap owner.
    pub owner: String,
    /// Bootstrap member name.
    pub name: String,
    /// Bootstrap member descriptor.
    pub descriptor: String,
}

/// Static constant substituted by a `\u{2}` recipe marker.
#[derive(Clone, Debug, PartialEq)]
pub enum ConcatConstant {
    /// Exact classfile string code units.
    String(JavaString),
    /// An `int` constant.
    Int(i32),
    /// A `long` constant.
    Long(i64),
    /// A `float` constant, retained as exact IEEE bits.
    Float(u32),
    /// A `double` constant, retained as exact IEEE bits.
    Double(u64),
}

/// Failure while admitting, linking, or executing a dynamic site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DynamicLinkError {
    /// The bootstrap protocol is outside the one admitted shape.
    UnadmittedBootstrap {
        /// Refused owner.
        owner: String,
        /// Refused name.
        name: String,
        /// Refused descriptor.
        descriptor: String,
    },
    /// The invoked type descriptor is malformed or contains an unsupported type.
    InvalidDescriptor(String),
    /// Recipe markers and supplied arguments/constants disagree.
    InvalidRecipe(String),
    /// A runtime argument disagrees with its linked descriptor.
    ArgumentMismatch {
        /// Zero-based argument index at the mismatch.
        index: usize,
        /// Linked parameter category or expected arity.
        expected: String,
    },
    /// Machine reentry for `Object.toString()` failed.
    ToString(String),
    /// Exact result storage could not be allocated.
    Allocation,
    /// Managed cache graph mutation failed.
    Cache(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Parameter {
    Boolean,
    Byte,
    Char,
    Short,
    Int,
    Long,
    Float,
    Double,
    Object,
}

#[derive(Clone, Debug)]
enum Piece {
    Literal(Vec<u16>),
    Argument(usize),
    Constant(usize),
}

/// Immutable result of validating one concat call site.
#[derive(Clone, Debug)]
pub struct LinkedStringConcat {
    parameters: Vec<Parameter>,
    pieces: Vec<Piece>,
    constants: Vec<ConcatConstant>,
}

impl LinkedStringConcat {
    /// Executes the linked recipe over exact UTF-16 code units.
    ///
    /// Object arguments reenter the embedding machine through `object_to_string`.
    /// A Java null reference, or a null result from `toString`, contributes `"null"`.
    pub fn execute<F>(
        &self,
        arguments: &[JvmValue],
        mut object_to_string: F,
    ) -> Result<JavaString, DynamicLinkError>
    where
        F: FnMut(JvmReference) -> Result<Option<JavaString>, String>,
    {
        if arguments.len() != self.parameters.len() {
            return Err(DynamicLinkError::ArgumentMismatch {
                index: arguments.len().min(self.parameters.len()),
                expected: format!("{} arguments", self.parameters.len()),
            });
        }
        let mut output = Vec::new();
        for piece in &self.pieces {
            match piece {
                Piece::Literal(units) => append(&mut output, units)?,
                Piece::Constant(index) => append_constant(&mut output, &self.constants[*index])?,
                Piece::Argument(index) => append_argument(
                    &mut output,
                    *index,
                    &self.parameters[*index],
                    &arguments[*index],
                    &mut object_to_string,
                )?,
            }
        }
        Ok(JavaString::new(CodeUnitString::from_code_units(output)))
    }
}

#[derive(Clone)]
struct DynamicCacheEntry {
    owner: Weak<ClassDefinition>,
    content_key: u64,
    revision: ClassSpaceRevision,
    linked: Arc<LinkedStringConcat>,
    _managed_value: ManagedHandle,
}

/// Lazy, occurrence-keyed string-concat linkage cache.
///
/// Entries are content/revision bound and use the managed cache role's ephemeron
/// edge, so neither this table nor the managed graph retains an unloaded class.
#[derive(Default)]
pub struct DynamicLinkCache {
    entries: Mutex<BTreeMap<(ClassDefinitionId, u32), DynamicCacheEntry>>,
}

impl DynamicLinkCache {
    /// Creates an empty lazy linkage cache.
    pub fn new() -> Self {
        Self::default()
    }

    /// Admits and links a site on first instruction execution, then reuses the
    /// result only while classfile content and class-space revision still match.
    #[allow(clippy::too_many_arguments)]
    pub fn link(
        &self,
        heap: &mut JvmHeap,
        cache: ManagedHandle,
        owner_handle: ManagedHandle,
        owner: &Arc<ClassDefinition>,
        revision: ClassSpaceRevision,
        occurrence: u32,
        bootstrap: &DynamicBootstrap,
        invoked_descriptor: &str,
        recipe: &JavaString,
        constants: &[ConcatConstant],
    ) -> Result<Arc<LinkedStringConcat>, DynamicLinkError> {
        admit(bootstrap)?;
        let key = (owner.id().clone(), occurrence);
        let mut entries = self.entries();
        entries.retain(|_, entry| entry.owner.strong_count() != 0);
        if let Some(entry) = entries.get(&key)
            && entry.content_key == owner.id().content_key()
            && entry.revision == revision
        {
            return Ok(Arc::clone(&entry.linked));
        }
        let linked = Arc::new(link(invoked_descriptor, recipe, constants)?);
        let managed_value = heap
            .allocate(JvmRole::Cache)
            .map_err(|error| DynamicLinkError::Cache(error.to_string()))?;
        heap.ephemeron(cache, JvmEdge::DerivedEntry, owner_handle, managed_value)
            .map_err(|error: JvmGraphError| DynamicLinkError::Cache(format!("{error:?}")))?;
        entries.insert(
            key,
            DynamicCacheEntry {
                owner: Arc::downgrade(owner),
                content_key: owner.id().content_key(),
                revision,
                linked: Arc::clone(&linked),
                _managed_value: managed_value,
            },
        );
        Ok(linked)
    }

    /// Returns the count of entries whose owning class remains live.
    pub fn live_len(&self) -> usize {
        let mut entries = self.entries();
        entries.retain(|_, entry| entry.owner.strong_count() != 0);
        entries.len()
    }

    fn entries(&self) -> MutexGuard<'_, BTreeMap<(ClassDefinitionId, u32), DynamicCacheEntry>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn admit(bootstrap: &DynamicBootstrap) -> Result<(), DynamicLinkError> {
    if bootstrap.owner == STRING_CONCAT_BOOTSTRAP_OWNER
        && bootstrap.name == STRING_CONCAT_BOOTSTRAP_NAME
        && bootstrap.descriptor == STRING_CONCAT_BOOTSTRAP_DESCRIPTOR
    {
        Ok(())
    } else {
        Err(DynamicLinkError::UnadmittedBootstrap {
            owner: bootstrap.owner.clone(),
            name: bootstrap.name.clone(),
            descriptor: bootstrap.descriptor.clone(),
        })
    }
}

fn link(
    descriptor: &str,
    recipe: &JavaString,
    constants: &[ConcatConstant],
) -> Result<LinkedStringConcat, DynamicLinkError> {
    let parameters = parameters(descriptor)?;
    let mut pieces = Vec::new();
    let mut literal = Vec::new();
    let mut argument = 0;
    let mut constant = 0;
    for unit in recipe.storage().code_units() {
        let marker = match unit {
            1 => Some(Piece::Argument(argument)),
            2 => Some(Piece::Constant(constant)),
            _ => None,
        };
        if let Some(piece) = marker {
            if !literal.is_empty() {
                pieces.push(Piece::Literal(std::mem::take(&mut literal)));
            }
            pieces.push(piece);
            if unit == 1 {
                argument += 1
            } else {
                constant += 1
            }
        } else {
            literal.push(unit);
        }
    }
    if !literal.is_empty() {
        pieces.push(Piece::Literal(literal));
    }
    if argument != parameters.len() || constant != constants.len() {
        return Err(DynamicLinkError::InvalidRecipe(format!(
            "recipe has {argument} argument and {constant} constant markers; site supplies {} arguments and {} constants",
            parameters.len(),
            constants.len()
        )));
    }
    Ok(LinkedStringConcat {
        parameters,
        pieces,
        constants: constants.to_vec(),
    })
}

fn parameters(descriptor: &str) -> Result<Vec<Parameter>, DynamicLinkError> {
    let bytes = descriptor.as_bytes();
    if bytes.first() != Some(&b'(') {
        return Err(DynamicLinkError::InvalidDescriptor(descriptor.into()));
    }
    let mut cursor = 1;
    let mut output = Vec::new();
    while bytes.get(cursor) != Some(&b')') {
        let parameter = match bytes.get(cursor).copied() {
            Some(b'Z') => {
                cursor += 1;
                Parameter::Boolean
            }
            Some(b'B') => {
                cursor += 1;
                Parameter::Byte
            }
            Some(b'C') => {
                cursor += 1;
                Parameter::Char
            }
            Some(b'S') => {
                cursor += 1;
                Parameter::Short
            }
            Some(b'I') => {
                cursor += 1;
                Parameter::Int
            }
            Some(b'J') => {
                cursor += 1;
                Parameter::Long
            }
            Some(b'F') => {
                cursor += 1;
                Parameter::Float
            }
            Some(b'D') => {
                cursor += 1;
                Parameter::Double
            }
            Some(b'L') => {
                cursor += 1;
                while !matches!(bytes.get(cursor), Some(b';') | None) {
                    cursor += 1;
                }
                if bytes.get(cursor) != Some(&b';') {
                    return Err(DynamicLinkError::InvalidDescriptor(descriptor.into()));
                }
                cursor += 1;
                Parameter::Object
            }
            Some(b'[') => {
                while bytes.get(cursor) == Some(&b'[') {
                    cursor += 1;
                }
                if bytes.get(cursor) == Some(&b'L') {
                    while !matches!(bytes.get(cursor), Some(b';') | None) {
                        cursor += 1;
                    }
                    if bytes.get(cursor) != Some(&b';') {
                        return Err(DynamicLinkError::InvalidDescriptor(descriptor.into()));
                    }
                } else if !matches!(
                    bytes.get(cursor),
                    Some(b'Z' | b'B' | b'C' | b'S' | b'I' | b'J' | b'F' | b'D')
                ) {
                    return Err(DynamicLinkError::InvalidDescriptor(descriptor.into()));
                }
                cursor += 1;
                Parameter::Object
            }
            _ => return Err(DynamicLinkError::InvalidDescriptor(descriptor.into())),
        };
        output.push(parameter);
    }
    if bytes.get(cursor + 1) != Some(&b'L')
        || !descriptor[cursor + 1..].starts_with("Ljava/lang/String;")
        || cursor + 19 != bytes.len()
    {
        return Err(DynamicLinkError::InvalidDescriptor(descriptor.into()));
    }
    Ok(output)
}

fn append_argument<F>(
    output: &mut Vec<u16>,
    index: usize,
    parameter: &Parameter,
    value: &JvmValue,
    object_to_string: &mut F,
) -> Result<(), DynamicLinkError>
where
    F: FnMut(JvmReference) -> Result<Option<JavaString>, String>,
{
    let text = match (parameter, value) {
        (Parameter::Boolean, JvmValue::Int(value)) => {
            if *value == 0 {
                "false".into()
            } else {
                "true".into()
            }
        }
        (Parameter::Byte | Parameter::Short | Parameter::Int, JvmValue::Int(value)) => {
            value.to_string()
        }
        (Parameter::Char, JvmValue::Int(value)) if (0..=u16::MAX as i32).contains(value) => {
            return append(output, &[*value as u16]);
        }
        (Parameter::Long, JvmValue::Long(value)) => value.to_string(),
        (Parameter::Float, JvmValue::Float(value)) => f32::from_bits(*value).to_string(),
        (Parameter::Double, JvmValue::Double(value)) => f64::from_bits(*value).to_string(),
        (Parameter::Object, JvmValue::Reference(reference)) if reference.handle().is_none() => {
            "null".into()
        }
        (Parameter::Object, JvmValue::Reference(reference)) => {
            return match object_to_string(*reference).map_err(DynamicLinkError::ToString)? {
                Some(string) => append(output, string.storage().as_code_units()),
                None => append_ascii(output, "null"),
            };
        }
        _ => {
            return Err(DynamicLinkError::ArgumentMismatch {
                index,
                expected: format!("{parameter:?}"),
            });
        }
    };
    append_ascii(output, &text)
}

fn append_constant(
    output: &mut Vec<u16>,
    constant: &ConcatConstant,
) -> Result<(), DynamicLinkError> {
    match constant {
        ConcatConstant::String(value) => append(output, value.storage().as_code_units()),
        ConcatConstant::Int(value) => append_ascii(output, &value.to_string()),
        ConcatConstant::Long(value) => append_ascii(output, &value.to_string()),
        ConcatConstant::Float(bits) => append_ascii(output, &f32::from_bits(*bits).to_string()),
        ConcatConstant::Double(bits) => append_ascii(output, &f64::from_bits(*bits).to_string()),
    }
}

fn append_ascii(output: &mut Vec<u16>, value: &str) -> Result<(), DynamicLinkError> {
    append(output, &value.encode_utf16().collect::<Vec<_>>())
}

fn append(output: &mut Vec<u16>, value: &[u16]) -> Result<(), DynamicLinkError> {
    output
        .try_reserve(value.len())
        .map_err(|_| DynamicLinkError::Allocation)?;
    output.extend_from_slice(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_lib_gc_tracing::CollectionLimits;

    fn string(units: &[u16]) -> JavaString {
        JavaString::new(CodeUnitString::from_code_units(units.to_vec()))
    }

    #[test]
    fn recipe_preserves_a_literal_lone_surrogate() {
        let recipe = string(&[b'[' as u16, 1, b']' as u16, 0xd800, 2]);
        let linked = link(
            "(I)Ljava/lang/String;",
            &recipe,
            &[ConcatConstant::String(string(&[0xdc00]))],
        )
        .unwrap();
        let result = linked
            .execute(&[JvmValue::Int(7)], |_| panic!("no object conversion"))
            .unwrap();
        assert_eq!(
            result.storage().as_code_units(),
            &[b'[' as u16, b'7' as u16, b']' as u16, 0xd800, 0xdc00]
        );
    }

    #[test]
    fn object_to_string_null_result_uses_jls_null_text() {
        let mut heap = JvmHeap::new(
            4,
            CollectionLimits {
                objects: 4,
                edges: 4,
                stack: 4,
                work: 16,
                clears: 4,
                finalizers: 0,
            },
        )
        .unwrap();
        let object = heap.allocate(JvmRole::Object).unwrap();
        let linked = link("(Ljava/lang/Object;)Ljava/lang/String;", &string(&[1]), &[]).unwrap();
        let mut calls = 0;
        let result = linked
            .execute(
                &[JvmValue::Reference(JvmReference::managed(object))],
                |_| {
                    calls += 1;
                    Ok(None)
                },
            )
            .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(
            result.storage().as_code_units(),
            &[b'n' as u16, b'u' as u16, b'l' as u16, b'l' as u16]
        );
    }

    #[test]
    fn unadmitted_bootstrap_is_refused_before_recipe_or_allocation() {
        let refused = DynamicBootstrap {
            owner: "evil/Bootstrap".into(),
            name: "run".into(),
            descriptor: "()V".into(),
        };
        let error = admit(&refused).unwrap_err();
        assert_eq!(
            error,
            DynamicLinkError::UnadmittedBootstrap {
                owner: "evil/Bootstrap".into(),
                name: "run".into(),
                descriptor: "()V".into(),
            }
        );
    }
}
