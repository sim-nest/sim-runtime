//! Checked, loader-neutral declarations for runtime classes.

use std::{collections::BTreeSet, error::Error, fmt};

use sim_kernel::{ClassId, ClassRef, ReadConstructorRef, Ref, ShapeRef, Symbol, Value};

/// Stable identity carried by a class declaration or parent reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClassIdentity {
    id: ClassId,
    symbol: Symbol,
}

impl ClassIdentity {
    /// Checks a numeric identity and its canonical (not merely display) name.
    pub fn checked(id: ClassId, symbol: Symbol) -> Result<Self, ClassDescriptorError> {
        validate_symbol(&symbol).map_err(|reason| ClassDescriptorError::InvalidIdentity {
            name: symbol.clone(),
            reason,
        })?;
        Ok(Self { id, symbol })
    }

    pub fn id(&self) -> ClassId {
        self.id
    }
    pub fn symbol(&self) -> &Symbol {
        &self.symbol
    }
}

/// A declared parent, explicitly separated by resolution state.
#[derive(Clone, Debug)]
pub enum DeclaredParent {
    /// The loader has supplied a concrete class object.
    Resolved {
        identity: ClassIdentity,
        class: ClassRef,
    },
    /// Resolution remains loader-owned; the reference is preserved verbatim.
    Unresolved {
        identity: ClassIdentity,
        reference: Ref,
    },
}

impl DeclaredParent {
    pub fn resolved(identity: ClassIdentity, class: ClassRef) -> Self {
        Self::Resolved { identity, class }
    }
    pub fn unresolved(identity: ClassIdentity, reference: Ref) -> Self {
        Self::Unresolved {
            identity,
            reference,
        }
    }
    pub fn identity(&self) -> &ClassIdentity {
        match self {
            Self::Resolved { identity, .. } | Self::Unresolved { identity, .. } => identity,
        }
    }
    pub fn unresolved_reference(&self) -> Option<&Ref> {
        match self {
            Self::Unresolved { reference, .. } => Some(reference),
            Self::Resolved { .. } => None,
        }
    }
    pub fn resolved_class(&self) -> Option<&ClassRef> {
        match self {
            Self::Resolved { class, .. } => Some(class),
            Self::Unresolved { .. } => None,
        }
    }
}

/// One named member and the Shape promised for its value.
#[derive(Clone, Debug)]
pub struct MemberShape {
    pub name: Symbol,
    pub shape: ShapeRef,
}

/// Open metadata: unknown keys are retained rather than interpreted here.
#[derive(Clone, Debug)]
pub struct OpenMetadataEntry {
    pub name: Symbol,
    pub value: Value,
}

/// Checked read-construct metadata.
#[derive(Clone, Debug)]
pub struct ReadConstruction {
    pub constructor: ReadConstructorRef,
    pub args_shape: ShapeRef,
}

/// Unchecked input consumed exactly once by [`ClassDescriptor::new`].
#[derive(Clone, Debug)]
pub struct ClassDescriptorInput {
    pub identity: ClassIdentity,
    pub parents: Vec<DeclaredParent>,
    pub constructor_shape: ShapeRef,
    pub instance_shape: ShapeRef,
    pub members: Vec<MemberShape>,
    pub read_construction: Option<ReadConstruction>,
    pub metadata: Vec<OpenMetadataEntry>,
}

/// Immutable class metadata after construction-time validation.
#[derive(Clone, Debug)]
pub struct ClassDescriptor {
    input: ClassDescriptorInput,
}

impl ClassDescriptor {
    pub fn new(input: ClassDescriptorInput) -> Result<Self, ClassDescriptorError> {
        validate_shape(Symbol::new("constructor"), &input.constructor_shape)?;
        validate_shape(Symbol::new("instance"), &input.instance_shape)?;

        let mut parents = BTreeSet::new();
        for parent in &input.parents {
            let identity = parent.identity();
            if identity.id == input.identity.id {
                return Err(ClassDescriptorError::SelfParent {
                    name: identity.symbol.clone(),
                });
            }
            if !parents.insert(identity.id) {
                return Err(ClassDescriptorError::DuplicateParent {
                    name: identity.symbol.clone(),
                });
            }
            if let DeclaredParent::Resolved { class, .. } = parent {
                let Some(actual) = class.object().as_class() else {
                    return Err(ClassDescriptorError::InvalidParent {
                        name: identity.symbol.clone(),
                        reason: "resolved value is not a class",
                    });
                };
                if actual.id() != identity.id || actual.symbol() != identity.symbol {
                    return Err(ClassDescriptorError::InvalidParent {
                        name: identity.symbol.clone(),
                        reason: "resolved class identity does not match its declaration",
                    });
                }
            }
        }

        let mut members = BTreeSet::new();
        for member in &input.members {
            validate_symbol(&member.name).map_err(|reason| {
                ClassDescriptorError::InvalidMember {
                    name: member.name.clone(),
                    reason,
                }
            })?;
            if !members.insert(member.name.clone()) {
                return Err(ClassDescriptorError::DuplicateMember {
                    name: member.name.clone(),
                });
            }
            validate_shape(member.name.clone(), &member.shape)?;
        }

        if let Some(read) = &input.read_construction {
            if read.constructor.object().as_read_constructor().is_none() {
                return Err(ClassDescriptorError::InvalidReadConstructor);
            }
            validate_shape(Symbol::new("read-constructor"), &read.args_shape)?;
        }

        let mut metadata = BTreeSet::new();
        for entry in &input.metadata {
            validate_symbol(&entry.name).map_err(|reason| {
                ClassDescriptorError::InvalidMetadata {
                    name: entry.name.clone(),
                    reason,
                }
            })?;
            if !metadata.insert(entry.name.clone()) {
                return Err(ClassDescriptorError::DuplicateMetadata {
                    name: entry.name.clone(),
                });
            }
        }
        Ok(Self { input })
    }

    pub fn identity(&self) -> &ClassIdentity {
        &self.input.identity
    }
    pub fn parents(&self) -> &[DeclaredParent] {
        &self.input.parents
    }
    pub fn constructor_shape(&self) -> &ShapeRef {
        &self.input.constructor_shape
    }
    pub fn instance_shape(&self) -> &ShapeRef {
        &self.input.instance_shape
    }
    pub fn members(&self) -> &[MemberShape] {
        &self.input.members
    }
    pub fn read_construction(&self) -> Option<&ReadConstruction> {
        self.input.read_construction.as_ref()
    }
    pub fn metadata(&self) -> &[OpenMetadataEntry] {
        &self.input.metadata
    }
}

fn validate_shape(name: Symbol, shape: &ShapeRef) -> Result<(), ClassDescriptorError> {
    if shape.object().as_shape().is_none() {
        return Err(ClassDescriptorError::MalformedShape { name });
    }
    Ok(())
}

fn validate_symbol(symbol: &Symbol) -> Result<(), &'static str> {
    if symbol.name.is_empty() {
        return Err("name is empty");
    }
    Symbol::checked(symbol.name.clone()).map_err(|_| "name is malformed")?;
    if let Some(namespace) = &symbol.namespace {
        if namespace.is_empty() {
            return Err("namespace is empty");
        }
        Symbol::checked(namespace.clone()).map_err(|_| "namespace is malformed")?;
    }
    Ok(())
}

/// Exact construction failure, retaining the offending declaration name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClassDescriptorError {
    InvalidIdentity { name: Symbol, reason: &'static str },
    InvalidParent { name: Symbol, reason: &'static str },
    DuplicateParent { name: Symbol },
    SelfParent { name: Symbol },
    InvalidMember { name: Symbol, reason: &'static str },
    DuplicateMember { name: Symbol },
    MalformedShape { name: Symbol },
    InvalidReadConstructor,
    InvalidMetadata { name: Symbol, reason: &'static str },
    DuplicateMetadata { name: Symbol },
}

impl fmt::Display for ClassDescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl Error for ClassDescriptorError {}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use sim_kernel::{
        Cx, DefaultFactory, Expr, MatchScore, NoopEvalPolicy, Shape, ShapeDoc, ShapeMatch,
    };

    use super::*;

    struct AnyShape;
    impl Shape for AnyShape {
        fn check_value(&self, _cx: &mut Cx, _value: Value) -> sim_kernel::Result<ShapeMatch> {
            Ok(ShapeMatch::accept(MatchScore::exact(1)))
        }
        fn check_expr(&self, _cx: &mut Cx, _expr: &Expr) -> sim_kernel::Result<ShapeMatch> {
            Ok(ShapeMatch::accept(MatchScore::exact(1)))
        }
        fn describe(&self, _cx: &mut Cx) -> sim_kernel::Result<ShapeDoc> {
            Ok(ShapeDoc::new("any"))
        }
    }

    fn cx() -> Cx {
        Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
    }

    fn shape(cx: &Cx) -> ShapeRef {
        cx.factory().opaque(Arc::new(AnyShape)).unwrap()
    }

    fn identity(id: u32, name: &str) -> ClassIdentity {
        ClassIdentity::checked(ClassId(id), Symbol::qualified("test", name)).unwrap()
    }

    fn input(cx: &Cx) -> ClassDescriptorInput {
        ClassDescriptorInput {
            identity: identity(40, "Child"),
            parents: Vec::new(),
            constructor_shape: shape(cx),
            instance_shape: shape(cx),
            members: Vec::new(),
            read_construction: None,
            metadata: Vec::new(),
        }
    }

    #[test]
    fn unresolved_parent_is_preserved_as_unresolved_typed_input() {
        let cx = cx();
        let unresolved = Ref::Symbol(Symbol::qualified("loader", "Parent"));
        let mut raw = input(&cx);
        raw.parents.push(DeclaredParent::unresolved(
            identity(41, "Parent"),
            unresolved.clone(),
        ));

        let descriptor = ClassDescriptor::new(raw).unwrap();
        assert_eq!(descriptor.parents().len(), 1);
        assert_eq!(
            descriptor.parents()[0].unresolved_reference(),
            Some(&unresolved)
        );
        assert!(descriptor.parents()[0].resolved_class().is_none());
    }

    #[test]
    fn duplicate_member_reports_the_offending_name() {
        let cx = cx();
        let mut raw = input(&cx);
        let name = Symbol::new("answer");
        raw.members.push(MemberShape {
            name: name.clone(),
            shape: shape(&cx),
        });
        raw.members.push(MemberShape {
            name: name.clone(),
            shape: shape(&cx),
        });
        assert_eq!(
            ClassDescriptor::new(raw).unwrap_err(),
            ClassDescriptorError::DuplicateMember { name }
        );
    }

    #[test]
    fn malformed_member_shape_reports_the_offending_name() {
        let cx = cx();
        let mut raw = input(&cx);
        let name = Symbol::new("broken");
        raw.members.push(MemberShape {
            name: name.clone(),
            shape: cx.factory().nil().unwrap(),
        });
        assert_eq!(
            ClassDescriptor::new(raw).unwrap_err(),
            ClassDescriptorError::MalformedShape { name }
        );
    }

    #[test]
    fn self_parent_reports_the_declared_parent_name() {
        let cx = cx();
        let mut raw = input(&cx);
        let own_identity = raw.identity.clone();
        let name = own_identity.symbol().clone();
        raw.parents.push(DeclaredParent::unresolved(
            own_identity,
            Ref::Symbol(name.clone()),
        ));
        assert_eq!(
            ClassDescriptor::new(raw).unwrap_err(),
            ClassDescriptorError::SelfParent { name }
        );
    }
}
