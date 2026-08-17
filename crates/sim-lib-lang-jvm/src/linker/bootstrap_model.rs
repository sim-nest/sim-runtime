/// A resolved constant-pool bootstrap argument ready for protocol validation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedBootstrapArgument {
    /// A method-type descriptor.
    MethodType(String),
    /// A method handle and its JVMS reference kind.
    MethodHandle {
        /// JVMS `reference_kind` from the resolved `CONSTANT_MethodHandle`.
        reference_kind: u8,
    },
    /// A marker-interface class internal name.
    Class(String),
    /// An integer flag or count.
    Integer(i32),
}
/// Fully validated lambda bootstrap payload.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LambdaBootstrapPlan {
    /// Erased SAM method descriptor.
    pub sam_method_type: String,
    /// Implementation method-handle reference kind.
    pub implementation_reference_kind: u8,
    /// Instantiated SAM method descriptor.
    pub instantiated_method_type: String,
    /// Marker interfaces requested by `altMetafactory`.
    pub marker_interfaces: Vec<String>,
    /// Additional bridge method descriptors.
    pub bridges: Vec<String>,
    /// Whether serialization support was requested.
    pub serializable: bool,
}

/// Declaration role of one member on a generated lambda class.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeneratedLambdaMemberRole {
    /// Capturing factory constructor whose arguments become immutable captures.
    FactoryConstructor,
    /// Concrete implementation of the functional interface's single abstract method.
    Sam,
    /// Erasure bridge requested by `altMetafactory`.
    Bridge,
}

/// Exact JVM declaration retained for a byte-free generated lambda member.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedLambdaMember {
    name: String,
    descriptor: String,
    role: GeneratedLambdaMemberRole,
}

impl GeneratedLambdaMember {
    /// JVM member name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// JVM method descriptor.
    pub fn descriptor(&self) -> &str {
        &self.descriptor
    }

    /// Linker role represented by this declaration.
    pub const fn role(&self) -> GeneratedLambdaMemberRole {
        self.role
    }
}

/// A loader-bound lambda class assembled directly from checked metadata.
///
/// Generated classes deliberately have no classfile expression, shell, or byte
/// storage. Their neutral face is the same checked `CLASS_2` descriptor used by
/// loaded classes, while invocation policy remains in the linker.
#[derive(Clone, Debug)]
pub struct GeneratedLambdaClass {
    binary_name: String,
    loader: crate::ClassLoaderId,
    mirror: ManagedHandle,
    descriptor: ClassDescriptor,
    members: Vec<GeneratedLambdaMember>,
    serializable: bool,
}

impl GeneratedLambdaClass {
    /// Stable generated binary name within the capturing loader.
    pub fn binary_name(&self) -> &str {
        &self.binary_name
    }

    /// Capturing loader which owns this class identity.
    pub const fn loader(&self) -> crate::ClassLoaderId {
        self.loader
    }

    /// Managed class-mirror node allocated for this class.
    pub const fn mirror(&self) -> ManagedHandle {
        self.mirror
    }

    /// Neutral browsable class metadata.
    pub fn descriptor(&self) -> &ClassDescriptor {
        &self.descriptor
    }

    /// JVM linker declarations in factory, SAM, then bridge order.
    pub fn members(&self) -> &[GeneratedLambdaMember] {
        &self.members
    }

    /// Whether the checked bootstrap requested Java lambda serialization.
    pub const fn serializable(&self) -> bool {
        self.serializable
    }

    /// Selects a callable lambda member by the JVM's exact name-and-descriptor key.
    ///
    /// Factory constructors are not callable through a lambda instance. Bridges
    /// otherwise receive no special ranking or fallback treatment.
    pub fn select_invocation_member(
        &self,
        name: &str,
        descriptor: &str,
    ) -> Option<SelectedLambdaMember> {
        self.members
            .iter()
            .find(|member| {
                member.role != GeneratedLambdaMemberRole::FactoryConstructor
                    && member.name == name
                    && member.descriptor == descriptor
            })
            .map(|member| SelectedLambdaMember {
                name: member.name.clone(),
                descriptor: member.descriptor.clone(),
                role: member.role,
            })
    }

    /// Projects this generated definition as an ordinary Shape-bearing class.
    pub fn class_value(
        &self,
        cx: &Cx,
        lineage_nodes: usize,
        lineage_work: usize,
    ) -> sim_kernel::Result<Value> {
        cx.factory().opaque(Arc::new(DescriptorClass::new(
            self.descriptor.clone(),
            |_cx: &mut Cx, _args| {
                Err(Error::Eval(
                    "generated JVM lambda instances require linker invocation".into(),
                ))
            },
            lineage_nodes,
            lineage_work,
        )))
    }
}

/// Selects a generated SAM/bridge and invokes its resolved implementation through
/// the caller's one JVM method pipeline.
///
/// A resumed call repeats selection against the immutable generated class and
/// passes the continuation through unchanged. Consequently lambda linkage cannot
/// reorder Java handlers, lose work evidence, or invent a distinct safepoint
/// contract.
#[allow(clippy::too_many_arguments)]
pub fn invoke_lambda_member<P: LambdaMethodPipeline>(
    pipeline: &mut P,
    class: &GeneratedLambdaClass,
    plan: &JvmFunctionPlan,
    implementation: &ResolvedDirectHandle,
    name: &str,
    descriptor: &str,
    captures: &[JvmValue],
    arguments: Vec<JvmValue>,
    resume: Option<P::Resume>,
) -> Result<LambdaInvocationOutcome<P::Resume, P::Exception>, InvocationError> {
    let member = class
        .select_invocation_member(name, descriptor)
        .ok_or(InvocationError::AbstractMethod)?;
    pipeline.invoke(LambdaMethodCall {
        member,
        implementation,
        adaptations: plan.body().adaptations(),
        captures,
        arguments,
        resume,
    })
}

/// Loader-local class space for byte-free lambda definitions.
#[derive(Default)]
pub struct GeneratedLambdaClassSpace {
    classes: BTreeMap<(crate::ClassLoaderId, String), GeneratedLambdaClassEntry>,
}

struct GeneratedLambdaClassEntry {
    owner: Weak<ClassDefinition>,
    class: Arc<GeneratedLambdaClass>,
}

impl GeneratedLambdaClassSpace {
    /// Creates an empty generated-class registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs or returns the stable class for one exact linkage site.
    #[allow(clippy::too_many_arguments)]
    pub fn define(
        &mut self,
        cx: &Cx,
        heap: &mut JvmHeap,
        loader: &ClassLoader,
        owner: &Arc<ClassDefinition>,
        site: &SiteKey,
        factory_descriptor: &str,
        functional: &FunctionalInterface,
        plan: &LambdaBootstrapPlan,
    ) -> Result<Arc<GeneratedLambdaClass>, GeneratedLambdaClassError> {
        let fingerprint = lambda_site_fingerprint(site);
        let binary_name = format!(
            "{}$$Lambda${fingerprint:016x}",
            site.class.binary_name().replace('/', ".")
        );
        let key = (loader.id(), binary_name.clone());
        self.classes
            .retain(|_, entry| entry.owner.strong_count() != 0);
        if let Some(existing) = self.classes.get(&key) {
            return Ok(existing.class.clone());
        }

        let shape: ShapeRef = cx
            .factory()
            .opaque(Arc::new(AnyShape))
            .map_err(|error| GeneratedLambdaClassError::Metadata(error.to_string()))?;
        let mut parent_names = vec![functional.interface.clone()];
        parent_names.extend(plan.marker_interfaces.iter().cloned());
        if plan.serializable
            && !parent_names
                .iter()
                .any(|name| name == "java.io.Serializable")
        {
            parent_names.push("java.io.Serializable".into());
        }
        let mut seen_parents = BTreeSet::new();
        parent_names.retain(|name| seen_parents.insert(name.clone()));
        let parents = parent_names
            .iter()
            .map(|name| {
                let identity = generated_identity(loader.id(), name, stable_text_hash(name))?;
                Ok(DeclaredParent::unresolved(
                    identity,
                    Ref::Symbol(Symbol::new(name.clone())),
                ))
            })
            .collect::<Result<Vec<_>, GeneratedLambdaClassError>>()?;

        let mut members = vec![
            GeneratedLambdaMember {
                name: "<init>".into(),
                descriptor: factory_descriptor.into(),
                role: GeneratedLambdaMemberRole::FactoryConstructor,
            },
            GeneratedLambdaMember {
                name: functional.method_name.clone(),
                descriptor: plan.instantiated_method_type.clone(),
                role: GeneratedLambdaMemberRole::Sam,
            },
        ];
        members.extend(
            plan.bridges
                .iter()
                .cloned()
                .map(|descriptor| GeneratedLambdaMember {
                    name: functional.method_name.clone(),
                    descriptor,
                    role: GeneratedLambdaMemberRole::Bridge,
                }),
        );
        let projected_members = members
            .iter()
            .enumerate()
            .map(|(index, _member)| MemberShape {
                // The ordinal preserves duplicate name/descriptor bridge declarations
                // while leaving the exact JVM identity in the linker metadata.
                name: Symbol::new(format!("lambda-member-{index}")),
                shape: shape.clone(),
            })
            .collect();
        let descriptor = ClassDescriptor::new(ClassDescriptorInput {
            identity: generated_identity(loader.id(), &binary_name, fingerprint)?,
            parents,
            constructor_shape: shape.clone(),
            instance_shape: shape,
            members: projected_members,
            read_construction: None,
            metadata: vec![
                OpenMetadataEntry {
                    name: Symbol::new("jvm.generated-kind"),
                    value: cx
                        .factory()
                        .string("lambda".into())
                        .map_err(|error| GeneratedLambdaClassError::Metadata(error.to_string()))?,
                },
                OpenMetadataEntry {
                    name: Symbol::new("jvm.factory-descriptor"),
                    value: cx
                        .factory()
                        .string(factory_descriptor.into())
                        .map_err(|error| GeneratedLambdaClassError::Metadata(error.to_string()))?,
                },
            ],
        })
        .map_err(|error| GeneratedLambdaClassError::Metadata(error.to_string()))?;
        let mirror = heap
            .allocate(crate::JvmRole::ClassMirror)
            .map_err(|error| GeneratedLambdaClassError::Managed(format!("{error:?}")))?;
        let generated = Arc::new(GeneratedLambdaClass {
            binary_name,
            loader: loader.id(),
            mirror,
            descriptor,
            members,
            serializable: plan.serializable,
        });
        self.classes.insert(
            key,
            GeneratedLambdaClassEntry {
                owner: Arc::downgrade(owner),
                class: generated.clone(),
            },
        );
        Ok(generated)
    }

    /// Returns generated classes for one loader in stable binary-name order.
    pub fn browse(
        &self,
        loader: crate::ClassLoaderId,
        limit: usize,
    ) -> Vec<Arc<GeneratedLambdaClass>> {
        self.classes
            .range((loader, String::new())..)
            .take_while(|((found, _), _)| *found == loader)
            .take(limit)
            .filter(|(_, entry)| entry.owner.strong_count() != 0)
            .map(|(_, entry)| entry.class.clone())
            .collect()
    }

    /// Number of generated classes whose capturing class remains live.
    pub fn live_len(&mut self) -> usize {
        self.classes
            .retain(|_, entry| entry.owner.strong_count() != 0);
        self.classes.len()
    }
}
