//! JVM-owned method selection and descriptor-driven machine transfers.

use std::{collections::BTreeSet, sync::Arc};

use sim_lib_machine::{CallTransfer, ReturnTransfer, TransferError};

use crate::{
    ClassDefinition, ClassDefinitionId, ClassLoader, ConstantResolution, ConstantResolutionKind,
    JavaMember, JvmValue,
};

/// The four linkage and selection modes of JVM invocation instructions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvocationKind {
    /// `invokestatic`.
    Static,
    /// `invokespecial`.
    Special,
    /// `invokevirtual`.
    Virtual,
    /// `invokeinterface`.
    Interface,
}

/// A selected Java declaration and its defining class.
#[derive(Clone, Debug)]
pub struct SelectedMethod {
    declaring_class: Arc<ClassDefinition>,
    method: JavaMember,
}

impl SelectedMethod {
    /// Class that declares the selected method body.
    pub fn declaring_class(&self) -> &Arc<ClassDefinition> {
        &self.declaring_class
    }

    /// Exact selected declaration. `ACC_BRIDGE` has no special semantics here.
    pub fn method(&self) -> &JavaMember {
        &self.method
    }
}

/// Stable Java invocation failures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvocationError {
    /// Symbolic resolution did not describe a method.
    NotMethod,
    /// A required class is not currently defined in the loader.
    MissingClass(String),
    /// Staticness is incompatible with the invocation instruction.
    StaticMismatch,
    /// Dynamic invocation did not supply a receiver class.
    MissingReceiver,
    /// No concrete declaration can be selected.
    AbstractMethod,
    /// Unrelated maximally-specific interface defaults remain.
    DefaultMethodConflict(Vec<ClassDefinitionId>),
    /// The method descriptor is malformed or uses an unsupported category.
    InvalidDescriptor,
    /// Supplied arguments do not match descriptor computational categories.
    ArgumentMismatch,
    /// A return value does not match the descriptor return category.
    ReturnMismatch,
    /// Shared machine transfer validation refused the packet.
    Transfer(TransferError),
}

/// Selects the Java target for a resolved invocation.
///
/// This function intentionally consumes only JVM metadata and never constructs
/// or invokes a generic SIM callable.
pub fn select_invocation(
    loader: &ClassLoader,
    resolved: &ConstantResolution,
    kind: InvocationKind,
    receiver_class: Option<&str>,
) -> Result<SelectedMethod, InvocationError> {
    if !matches!(
        resolved.kind,
        ConstantResolutionKind::Method | ConstantResolutionKind::InterfaceMethod
    ) {
        return Err(InvocationError::NotMethod);
    }
    let name = resolved.name.as_deref().ok_or(InvocationError::NotMethod)?;
    let descriptor = resolved
        .descriptor
        .as_deref()
        .ok_or(InvocationError::NotMethod)?;
    let declaring = load(loader, resolved.class.binary_name())?;
    let resolved_method = declaring
        .metadata()
        .select_method(name, descriptor)
        .cloned()
        .ok_or(InvocationError::AbstractMethod)?;

    match kind {
        InvocationKind::Static => {
            if !resolved_method.is_static() {
                return Err(InvocationError::StaticMismatch);
            }
            selected(declaring, resolved_method)
        }
        InvocationKind::Special => {
            if resolved_method.is_static() {
                return Err(InvocationError::StaticMismatch);
            }
            if resolved_method.is_abstract() {
                return Err(InvocationError::AbstractMethod);
            }
            selected(declaring, resolved_method)
        }
        InvocationKind::Virtual | InvocationKind::Interface => {
            if resolved_method.is_static() {
                return Err(InvocationError::StaticMismatch);
            }
            let receiver = load(
                loader,
                receiver_class.ok_or(InvocationError::MissingReceiver)?,
            )?;
            if let Some(target) = first_class_declaration(
                loader,
                receiver.clone(),
                name,
                descriptor,
                &mut BTreeSet::new(),
            )? && !target.1.is_abstract()
            {
                return selected(target.0, target.1);
            }
            select_default(loader, receiver, name, descriptor)
        }
    }
}

fn selected(
    class: Arc<ClassDefinition>,
    method: JavaMember,
) -> Result<SelectedMethod, InvocationError> {
    Ok(SelectedMethod {
        declaring_class: class,
        method,
    })
}

fn load(loader: &ClassLoader, name: &str) -> Result<Arc<ClassDefinition>, InvocationError> {
    loader
        .loaded(name)
        .map_err(|_| InvocationError::MissingClass(name.into()))?
        .ok_or_else(|| InvocationError::MissingClass(name.into()))
}

fn first_class_declaration(
    loader: &ClassLoader,
    class: Arc<ClassDefinition>,
    name: &str,
    descriptor: &str,
    visited: &mut BTreeSet<ClassDefinitionId>,
) -> Result<Option<(Arc<ClassDefinition>, JavaMember)>, InvocationError> {
    if !visited.insert(class.id().clone()) {
        return Ok(None);
    }
    if class.metadata().access_flags() & 0x0200 == 0 {
        if let Some(method) = class.metadata().select_method(name, descriptor)
            && !method.is_static()
        {
            return Ok(Some((class.clone(), method.clone())));
        }
        if let Some(parent) = class.metadata().resolution().direct_parents().first() {
            let parent = load(loader, parent)?;
            if parent.metadata().access_flags() & 0x0200 == 0 {
                return first_class_declaration(loader, parent, name, descriptor, visited);
            }
        }
    }
    Ok(None)
}

fn select_default(
    loader: &ClassLoader,
    receiver: Arc<ClassDefinition>,
    name: &str,
    descriptor: &str,
) -> Result<SelectedMethod, InvocationError> {
    let mut interfaces = Vec::new();
    collect_interfaces(loader, receiver, &mut BTreeSet::new(), &mut interfaces)?;
    let mut candidates = interfaces
        .iter()
        .filter_map(|class| {
            class
                .metadata()
                .select_method(name, descriptor)
                .and_then(|method| {
                    (!method.is_static() && !method.is_abstract())
                        .then(|| (class.clone(), method.clone()))
                })
        })
        .collect::<Vec<_>>();
    candidates.retain(|(candidate, _)| {
        !interfaces.iter().any(|other| {
            other.id() != candidate.id()
                && is_subinterface(
                    loader,
                    other,
                    candidate.id().binary_name(),
                    &mut BTreeSet::new(),
                )
                .unwrap_or(false)
                && other.metadata().select_method(name, descriptor).is_some()
        })
    });
    match candidates.len() {
        0 => Err(InvocationError::AbstractMethod),
        1 => {
            let (class, method) = candidates.pop().expect("one candidate");
            selected(class, method)
        }
        _ => Err(InvocationError::DefaultMethodConflict(
            candidates
                .into_iter()
                .map(|(class, _)| class.id().clone())
                .collect(),
        )),
    }
}

fn collect_interfaces(
    loader: &ClassLoader,
    class: Arc<ClassDefinition>,
    visited: &mut BTreeSet<ClassDefinitionId>,
    output: &mut Vec<Arc<ClassDefinition>>,
) -> Result<(), InvocationError> {
    if !visited.insert(class.id().clone()) {
        return Ok(());
    }
    for parent_name in class.metadata().resolution().direct_parents() {
        let parent = load(loader, parent_name)?;
        if parent.metadata().access_flags() & 0x0200 != 0 {
            output.push(parent.clone());
        }
        collect_interfaces(loader, parent, visited, output)?;
    }
    Ok(())
}

fn is_subinterface(
    loader: &ClassLoader,
    class: &Arc<ClassDefinition>,
    target: &str,
    visited: &mut BTreeSet<ClassDefinitionId>,
) -> Result<bool, InvocationError> {
    if !visited.insert(class.id().clone()) {
        return Ok(false);
    }
    for parent in class.metadata().resolution().direct_parents() {
        if parent == target || is_subinterface(loader, &load(loader, parent)?, target, visited)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Category {
    Int,
    Float,
    Long,
    Double,
    Reference,
    Void,
}

/// Builds a width-checked machine call packet from descriptor arguments.
pub fn call_transfer(
    selected: SelectedMethod,
    receiver: Option<JvmValue>,
    arguments: Vec<JvmValue>,
) -> Result<CallTransfer<JvmValue, SelectedMethod>, InvocationError> {
    let (parameters, _) = parse_descriptor(selected.method.descriptor())?;
    if parameters.len() != arguments.len()
        || !parameters
            .iter()
            .zip(&arguments)
            .all(|(category, value)| matches_category(*category, value))
    {
        return Err(InvocationError::ArgumentMismatch);
    }
    let mut values = Vec::with_capacity(arguments.len() + usize::from(receiver.is_some()));
    if selected.method.is_static() {
        if receiver.is_some() {
            return Err(InvocationError::ArgumentMismatch);
        }
    } else {
        let receiver = receiver.ok_or(InvocationError::ArgumentMismatch)?;
        if !matches!(receiver, JvmValue::Reference(_)) {
            return Err(InvocationError::ArgumentMismatch);
        }
        values.push(receiver);
    }
    values.extend(arguments);
    let widths = values.iter().map(JvmValue::logical_width).collect();
    CallTransfer::new(values, widths, selected).map_err(InvocationError::Transfer)
}

/// Builds the width-checked machine return packet for every JVM return form.
pub fn return_transfer(
    descriptor: &str,
    value: Option<JvmValue>,
) -> Result<ReturnTransfer<JvmValue>, InvocationError> {
    let (_, category) = parse_descriptor(descriptor)?;
    let values = match (category, value) {
        (Category::Void, None) => Vec::new(),
        (expected, Some(value)) if matches_category(expected, &value) => vec![value],
        _ => return Err(InvocationError::ReturnMismatch),
    };
    let widths = values.iter().map(JvmValue::logical_width).collect();
    ReturnTransfer::new(values, widths).map_err(InvocationError::Transfer)
}

fn matches_category(category: Category, value: &JvmValue) -> bool {
    matches!(
        (category, value),
        (Category::Int, JvmValue::Int(_))
            | (Category::Float, JvmValue::Float(_))
            | (Category::Long, JvmValue::Long(_))
            | (Category::Double, JvmValue::Double(_))
            | (Category::Reference, JvmValue::Reference(_))
    )
}

fn parse_descriptor(descriptor: &str) -> Result<(Vec<Category>, Category), InvocationError> {
    let bytes = descriptor.as_bytes();
    if bytes.first() != Some(&b'(') {
        return Err(InvocationError::InvalidDescriptor);
    }
    let mut cursor = 1;
    let mut parameters = Vec::new();
    while bytes.get(cursor) != Some(&b')') {
        parameters.push(parse_type(bytes, &mut cursor, false)?);
    }
    cursor += 1;
    let result = parse_type(bytes, &mut cursor, true)?;
    if cursor != bytes.len() {
        return Err(InvocationError::InvalidDescriptor);
    }
    Ok((parameters, result))
}

fn parse_type(
    bytes: &[u8],
    cursor: &mut usize,
    allow_void: bool,
) -> Result<Category, InvocationError> {
    let byte = *bytes
        .get(*cursor)
        .ok_or(InvocationError::InvalidDescriptor)?;
    *cursor += 1;
    match byte {
        b'B' | b'C' | b'I' | b'S' | b'Z' => Ok(Category::Int),
        b'F' => Ok(Category::Float),
        b'J' => Ok(Category::Long),
        b'D' => Ok(Category::Double),
        b'V' if allow_void => Ok(Category::Void),
        b'L' => {
            let rest = bytes
                .get(*cursor..)
                .ok_or(InvocationError::InvalidDescriptor)?;
            let end = rest
                .iter()
                .position(|byte| *byte == b';')
                .ok_or(InvocationError::InvalidDescriptor)?;
            if end == 0 {
                return Err(InvocationError::InvalidDescriptor);
            }
            *cursor += end + 1;
            Ok(Category::Reference)
        }
        b'[' => {
            while bytes.get(*cursor) == Some(&b'[') {
                *cursor += 1;
            }
            parse_type(bytes, cursor, false)?;
            Ok(Category::Reference)
        }
        _ => Err(InvocationError::InvalidDescriptor),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy};

    fn insert(
        cx: &Cx,
        loader: &ClassLoader,
        name: &str,
        parents: &[&str],
        flags: u16,
        methods: &[(&str, &str, u16)],
    ) -> Arc<ClassDefinition> {
        let metadata = crate::JavaClassMetadata::test_class(cx, name, parents, flags, methods);
        let definition = Arc::new(ClassDefinition::test_definition(metadata));
        loader.test_insert(definition.clone());
        definition
    }

    fn resolution(class: &ClassDefinition, kind: ConstantResolutionKind) -> ConstantResolution {
        ConstantResolution {
            kind,
            class: class.id().clone(),
            name: Some("run".into()),
            descriptor: Some("(IJLjava/lang/Object;)[I".into()),
        }
    }

    #[test]
    fn maximally_specific_default_wins_over_inherited_abstract() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let loader = ClassLoader::new(1024);
        insert(&cx, &loader, "java.lang.Object", &[], 0x0001, &[]);
        let base = insert(
            &cx,
            &loader,
            "p.Base",
            &[],
            0x0201,
            &[("run", "()V", 0x0401)],
        );
        insert(
            &cx,
            &loader,
            "p.Specific",
            &["p.Base"],
            0x0201,
            &[("run", "()V", 0x0001)],
        );
        insert(
            &cx,
            &loader,
            "p.Receiver",
            &["java.lang.Object", "p.Specific"],
            0x0001,
            &[],
        );
        let mut resolved = resolution(&base, ConstantResolutionKind::InterfaceMethod);
        resolved.descriptor = Some("()V".into());
        let selected = select_invocation(
            &loader,
            &resolved,
            InvocationKind::Interface,
            Some("p.Receiver"),
        )
        .unwrap();
        assert_eq!(selected.declaring_class().id().binary_name(), "p.Specific");
    }

    #[test]
    fn unrelated_defaults_report_the_specified_conflict() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let loader = ClassLoader::new(1024);
        insert(&cx, &loader, "java.lang.Object", &[], 0x0001, &[]);
        let left = insert(
            &cx,
            &loader,
            "p.Left",
            &[],
            0x0201,
            &[("run", "()V", 0x0001)],
        );
        insert(
            &cx,
            &loader,
            "p.Right",
            &[],
            0x0201,
            &[("run", "()V", 0x0001)],
        );
        insert(
            &cx,
            &loader,
            "p.Receiver",
            &["java.lang.Object", "p.Left", "p.Right"],
            0x0001,
            &[],
        );
        let mut resolved = resolution(&left, ConstantResolutionKind::InterfaceMethod);
        resolved.descriptor = Some("()V".into());
        let error = select_invocation(
            &loader,
            &resolved,
            InvocationKind::Interface,
            Some("p.Receiver"),
        )
        .unwrap_err();
        let InvocationError::DefaultMethodConflict(classes) = error else {
            panic!("unexpected error: {error:?}")
        };
        assert_eq!(
            classes
                .iter()
                .map(ClassDefinitionId::binary_name)
                .collect::<Vec<_>>(),
            ["p.Left", "p.Right"]
        );
    }

    #[test]
    fn bridge_is_selected_normally_and_all_transfer_forms_are_width_exact() {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        let loader = ClassLoader::new(1024);
        let class = insert(
            &cx,
            &loader,
            "p.Bridge",
            &[],
            0x0001,
            &[("run", "(IJLjava/lang/Object;)[I", 0x0041)],
        );
        let selected = select_invocation(
            &loader,
            &resolution(&class, ConstantResolutionKind::Method),
            InvocationKind::Virtual,
            Some("p.Bridge"),
        )
        .unwrap();
        assert!(selected.method().is_bridge());
        let packet = call_transfer(
            selected,
            Some(JvmValue::Reference(crate::JvmReference::NULL)),
            vec![
                JvmValue::Int(1),
                JvmValue::Long(2),
                JvmValue::Reference(crate::JvmReference::NULL),
            ],
        )
        .unwrap();
        assert_eq!(packet.widths, [1, 1, 2, 1]);
        assert!(return_transfer("()V", None).unwrap().values.is_empty());
        assert_eq!(
            return_transfer("()I", Some(JvmValue::Int(1)))
                .unwrap()
                .widths,
            [1]
        );
        assert_eq!(
            return_transfer("()J", Some(JvmValue::Long(1)))
                .unwrap()
                .widths,
            [2]
        );
        assert_eq!(
            return_transfer("()F", Some(JvmValue::Float(1)))
                .unwrap()
                .widths,
            [1]
        );
        assert_eq!(
            return_transfer("()D", Some(JvmValue::Double(1)))
                .unwrap()
                .widths,
            [2]
        );
        assert_eq!(
            return_transfer(
                "()Ljava/lang/Object;",
                Some(JvmValue::Reference(crate::JvmReference::NULL))
            )
            .unwrap()
            .widths,
            [1]
        );
    }

    #[test]
    fn invocation_source_has_no_generic_dispatch_path() {
        let source = include_str!("invocation.rs");
        assert!(!source.contains(concat!("sim_lib_", "dispatch")));
        assert!(!source.contains(concat!("sim-lib-", "dispatch")));
    }
}
