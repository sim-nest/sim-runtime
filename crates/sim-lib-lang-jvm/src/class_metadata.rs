//! JVM policy retained beside the language-neutral `CLASS_2` projection.

use std::sync::Arc;

use sim_codec_classfile::{ClassIndex, ClassShell, Constant, Utf8Index, ValidatedClassShell};
use sim_kernel::{ClassId, Cx, Error, Result, ShapeRef, Symbol, Value};
use sim_lib_class::{
    ClassDescriptor, ClassDescriptorInput, ClassIdentity, DeclaredParent, DescriptorClass,
    MemberShape,
};
use sim_shape::AnyShape;

use crate::{ClassDefinitionId, ClassLoaderId};

/// JVM-owned declaration for a field or method. The descriptor remains part of
/// identity because Java permits method overloads with a shared name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JavaMember {
    name: String,
    descriptor: String,
    access_flags: u16,
    kind: JavaMemberKind,
}

impl JavaMember {
    #[cfg(test)]
    pub(crate) fn test_field(name: &str, descriptor: &str, access_flags: u16) -> Self {
        Self {
            name: name.into(),
            descriptor: descriptor.into(),
            access_flags,
            kind: JavaMemberKind::Field,
        }
    }

    /// JVM source name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// JVM field or method descriptor.
    pub fn descriptor(&self) -> &str {
        &self.descriptor
    }
    /// Uninterpreted JVMS access flags.
    pub fn access_flags(&self) -> u16 {
        self.access_flags
    }
    /// Declaration kind.
    pub fn kind(&self) -> JavaMemberKind {
        self.kind
    }

    /// Whether this declaration carries the JVM `ACC_STATIC` flag.
    pub const fn is_static(&self) -> bool {
        self.access_flags & 0x0008 != 0
    }

    /// Whether this declaration carries the JVM `ACC_FINAL` flag.
    pub const fn is_final(&self) -> bool {
        self.access_flags & 0x0010 != 0
    }

    /// Whether this declaration carries the JVM `ACC_ABSTRACT` flag.
    pub const fn is_abstract(&self) -> bool {
        self.access_flags & 0x0400 != 0
    }

    /// Whether this declaration carries the JVM `ACC_BRIDGE` flag.
    ///
    /// Bridge status is descriptive only: selection deliberately does not
    /// special-case it because bridges are ordinary methods in the JVM.
    pub const fn is_bridge(&self) -> bool {
        self.access_flags & 0x0040 != 0
    }
}

/// Kind of a JVM member declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JavaMemberKind {
    /// A declared Java field.
    Field,
    /// A declared Java method or constructor.
    Method,
}

/// Resolution evidence deliberately retained outside the neutral descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JavaResolutionEvidence {
    loader: ClassLoaderId,
    binary_name: String,
    direct_parents: Vec<String>,
}

/// Result of a JVM metadata hierarchy walk with an explicit node allowance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JavaHierarchyCheck {
    /// The target identity was found.
    Match,
    /// The retained declared lineage does not name the target.
    NoMatch,
    /// More nodes were required than the caller allowed.
    BudgetExhausted {
        /// Caller-supplied maximum visited nodes.
        limit: usize,
    },
}

impl JavaResolutionEvidence {
    /// Loader namespace in which names must be resolved.
    pub fn loader(&self) -> ClassLoaderId {
        self.loader
    }
    /// Defined binary name.
    pub fn binary_name(&self) -> &str {
        &self.binary_name
    }
    /// Declared superclass followed by directly implemented interfaces.
    pub fn direct_parents(&self) -> &[String] {
        &self.direct_parents
    }
}

/// A loaded Java class: a neutral browsable descriptor plus JVM-only policy.
#[derive(Clone, Debug)]
pub struct JavaClassMetadata {
    descriptor: ClassDescriptor,
    access_flags: u16,
    resolution: JavaResolutionEvidence,
    members: Vec<JavaMember>,
    array_component: Option<Arc<JavaClassMetadata>>,
}

impl JavaClassMetadata {
    #[cfg(test)]
    pub(crate) fn test_identity(cx: &Cx, binary_name: &str, direct_parents: &[&str]) -> Self {
        let shape: ShapeRef = cx.factory().opaque(Arc::new(AnyShape)).unwrap();
        Self {
            descriptor: ClassDescriptor::new(ClassDescriptorInput {
                identity: ClassIdentity::checked(
                    derived_class_id(ClassLoaderId(1), binary_name),
                    Symbol::new(binary_name),
                )
                .unwrap(),
                parents: Vec::new(),
                constructor_shape: shape.clone(),
                instance_shape: shape,
                members: Vec::new(),
                read_construction: None,
                metadata: Vec::new(),
            })
            .unwrap(),
            access_flags: 0,
            resolution: JavaResolutionEvidence {
                loader: ClassLoaderId(1),
                binary_name: binary_name.into(),
                direct_parents: direct_parents.iter().map(|name| (*name).into()).collect(),
            },
            members: Vec::new(),
            array_component: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn test_class(
        cx: &Cx,
        binary_name: &str,
        direct_parents: &[&str],
        access_flags: u16,
        methods: &[(&str, &str, u16)],
    ) -> Self {
        let mut metadata = Self::test_identity(cx, binary_name, direct_parents);
        metadata.access_flags = access_flags;
        metadata.members = methods
            .iter()
            .map(|(name, descriptor, access_flags)| JavaMember {
                name: (*name).into(),
                descriptor: (*descriptor).into(),
                access_flags: *access_flags,
                kind: JavaMemberKind::Method,
            })
            .collect();
        metadata
    }

    pub(crate) fn from_shell(
        cx: &Cx,
        definition: &ClassDefinitionId,
        shell: &ClassShell,
        validated: &ValidatedClassShell,
    ) -> Result<Self> {
        let shape: ShapeRef = cx.factory().opaque(Arc::new(AnyShape))?;
        let mut parent_names = Vec::new();
        if let Some(parent) = validated.super_class {
            parent_names.push(class_name(shell, parent)?);
        }
        for parent in &validated.interfaces {
            parent_names.push(class_name(shell, *parent)?);
        }
        let parents = parent_names
            .iter()
            .map(|name| {
                let id = derived_class_id(definition.loader(), name);
                Ok(DeclaredParent::unresolved(
                    ClassIdentity::checked(id, Symbol::new(name.clone())).map_err(class_error)?,
                    sim_kernel::Ref::Symbol(Symbol::new(name.clone())),
                ))
            })
            .collect::<Result<Vec<_>>>()?;

        let mut members = Vec::new();
        for field in &validated.fields {
            members.push(member(
                shell,
                field.name,
                field.descriptor,
                JavaMemberKind::Field,
                0,
            )?);
        }
        for (raw, method) in shell.methods.iter().zip(&validated.methods) {
            members.push(member(
                shell,
                method.name,
                method.descriptor,
                JavaMemberKind::Method,
                raw.access_flags,
            )?);
        }
        for (raw, field) in shell.fields.iter().zip(
            members
                .iter_mut()
                .filter(|m| m.kind == JavaMemberKind::Field),
        ) {
            field.access_flags = raw.access_flags;
        }
        let projected_members = members
            .iter()
            .map(|member| MemberShape {
                name: Symbol::new(format!("{}:{}", member.name, member.descriptor)),
                shape: shape.clone(),
            })
            .collect();
        let descriptor = ClassDescriptor::new(ClassDescriptorInput {
            identity: ClassIdentity::checked(
                derived_class_id(definition.loader(), definition.binary_name()),
                Symbol::new(definition.binary_name().to_owned()),
            )
            .map_err(class_error)?,
            parents,
            constructor_shape: shape.clone(),
            instance_shape: shape,
            members: projected_members,
            read_construction: None,
            metadata: Vec::new(),
        })
        .map_err(class_error)?;
        Ok(Self {
            descriptor,
            access_flags: shell.access_flags,
            resolution: JavaResolutionEvidence {
                loader: definition.loader(),
                binary_name: definition.binary_name().to_owned(),
                direct_parents: parent_names,
            },
            members,
            array_component: None,
        })
    }

    /// Neutral, browsable `CLASS_2` face.
    pub fn descriptor(&self) -> &ClassDescriptor {
        &self.descriptor
    }
    /// Raw JVM class access flags, outside the neutral face.
    pub fn access_flags(&self) -> u16 {
        self.access_flags
    }
    /// Loader and unresolved-name evidence, outside the neutral face.
    pub fn resolution(&self) -> &JavaResolutionEvidence {
        &self.resolution
    }
    /// JVM declarations in classfile order.
    pub fn members(&self) -> &[JavaMember] {
        &self.members
    }

    /// Selects a declared Java method by its exact JVMS name and descriptor.
    /// This API returns metadata, not a SIM callable, so generic dispatch cannot
    /// participate in Java method selection.
    pub fn select_method(&self, name: &str, descriptor: &str) -> Option<&JavaMember> {
        self.members.iter().find(|member| {
            member.kind == JavaMemberKind::Method
                && member.name == name
                && member.descriptor == descriptor
        })
    }

    /// Produces a stable derived array class in the same loader namespace.
    pub fn array_of(cx: &Cx, component: Arc<Self>) -> Result<Self> {
        let binary_name = format!("[{}", component.resolution.binary_name);
        let shape: ShapeRef = cx.factory().opaque(Arc::new(AnyShape))?;
        let descriptor = ClassDescriptor::new(ClassDescriptorInput {
            identity: ClassIdentity::checked(
                derived_class_id(component.resolution.loader, &binary_name),
                Symbol::new(binary_name.clone()),
            )
            .map_err(class_error)?,
            parents: Vec::new(),
            constructor_shape: shape.clone(),
            instance_shape: shape,
            members: Vec::new(),
            read_construction: None,
            metadata: Vec::new(),
        })
        .map_err(class_error)?;
        Ok(Self {
            descriptor,
            access_flags: 0x0010 | 0x0400,
            resolution: JavaResolutionEvidence {
                loader: component.resolution.loader,
                binary_name,
                direct_parents: vec![
                    "java.lang.Object".into(),
                    "java.lang.Cloneable".into(),
                    "java.io.Serializable".into(),
                ],
            },
            members: Vec::new(),
            array_component: Some(component),
        })
    }

    /// Component metadata for a derived array class.
    pub fn array_component(&self) -> Option<&Arc<Self>> {
        self.array_component.as_ref()
    }

    /// Checks retained JVM lineage without invoking the generic class dispatch
    /// relation. Array component traversal consumes the same finite allowance.
    pub fn is_assignable_to_binary_name(
        &self,
        expected: &str,
        node_limit: usize,
    ) -> JavaHierarchyCheck {
        if node_limit == 0 {
            return JavaHierarchyCheck::BudgetExhausted { limit: node_limit };
        }
        if self.resolution.binary_name == expected
            || self
                .resolution
                .direct_parents
                .iter()
                .any(|parent| parent == expected)
        {
            return JavaHierarchyCheck::Match;
        }
        let Some(component) = &self.array_component else {
            return JavaHierarchyCheck::NoMatch;
        };
        let Some(expected_component) = expected.strip_prefix('[') else {
            return JavaHierarchyCheck::NoMatch;
        };
        component.is_assignable_to_binary_name(expected_component, node_limit - 1)
    }

    /// Creates the ordinary SIM class object. Construction is refused because
    /// JVM allocation/invocation remains owned by later JVM execution policy.
    pub fn class_value(&self, cx: &Cx, lineage_nodes: usize, lineage_work: usize) -> Result<Value> {
        cx.factory().opaque(Arc::new(DescriptorClass::new(
            self.descriptor.clone(),
            |_cx: &mut Cx, _args| {
                Err(Error::Eval(
                    "Java classes are not generic SIM dispatch targets".into(),
                ))
            },
            lineage_nodes,
            lineage_work,
        )))
    }
}

fn member(
    shell: &ClassShell,
    name: Utf8Index,
    descriptor: Utf8Index,
    kind: JavaMemberKind,
    access_flags: u16,
) -> Result<JavaMember> {
    Ok(JavaMember {
        name: utf8(shell, name)?,
        descriptor: utf8(shell, descriptor)?,
        access_flags,
        kind,
    })
}

fn utf8(shell: &ClassShell, index: Utf8Index) -> Result<String> {
    let Constant::Utf8(value) = shell
        .constant_pool
        .entry(index.0, index.0)
        .map_err(class_error)?
    else {
        return Err(Error::Eval("validated UTF-8 index changed type".into()));
    };
    String::from_utf16(value.as_code_units())
        .map_err(|_| Error::Eval("class metadata is not valid Unicode".into()))
}

fn class_name(shell: &ClassShell, index: ClassIndex) -> Result<String> {
    let Constant::Class { name_index } = shell
        .constant_pool
        .entry(index.0, index.0)
        .map_err(class_error)?
    else {
        return Err(Error::Eval("validated class index changed type".into()));
    };
    utf8(shell, Utf8Index(*name_index)).map(|name| name.replace('/', "."))
}

fn derived_class_id(loader: ClassLoaderId, name: &str) -> ClassId {
    let mut hash = 0x811c9dc5_u32 ^ (loader.0 as u32) ^ ((loader.0 >> 32) as u32);
    for byte in name.bytes() {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x01000193);
    }
    ClassId(hash.max(1))
}

fn class_error(error: impl std::fmt::Display) -> Error {
    Error::Eval(error.to_string())
}
