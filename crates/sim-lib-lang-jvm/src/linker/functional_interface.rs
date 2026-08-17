/// Located evidence that a loaded interface has one Java single abstract method.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FunctionalInterface {
    /// Interface named by the invokedynamic call-site return type.
    pub interface: String,
    /// Exact erased method name.
    pub method_name: String,
    /// Exact erased SAM descriptor.
    pub method_descriptor: String,
    /// Loaded interfaces consulted, in deterministic traversal order.
    pub lineage: Vec<String>,
}
/// Fail-closed functional-interface and metafactory type validation error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FunctionalInterfaceError {
    /// SIM-to-Java interop was refused before generated class construction.
    InteropRefused(String),
    /// A required class is absent from the caller's already-loaded view.
    MissingClass(String),
    /// The call-site return or marker type is not an interface.
    NotInterface(String),
    /// A marker interface is not accessible to the capturing linkage site.
    InaccessibleInterface(String),
    /// The bounded interface walk would consult more nodes than allowed.
    HierarchyBudgetExhausted {
        /// Maximum loaded interface nodes the caller admitted.
        limit: usize,
    },
    /// No abstract method remains after Java `Object` exclusions.
    NoAbstractMethod {
        /// Interface whose inherited declarations were inspected.
        interface: String,
    },
    /// More than one unrelated abstract method signature remains.
    MultipleAbstractMethods {
        /// Deterministically ordered incompatible method identities.
        methods: Vec<String>,
    },
    /// A method descriptor or the invoked type is structurally invalid.
    InvalidDescriptor(String),
    /// Bootstrap and discovered SAM types disagree.
    SamTypeMismatch {
        /// Descriptor discovered from the interface hierarchy.
        discovered: String,
        /// Descriptor supplied by the bootstrap payload.
        supplied: String,
    },
    /// The instantiated or implementation method cannot implement the SAM.
    IncompatibleMethodType {
        /// Type-bearing bootstrap input that failed adaptation.
        role: &'static str,
        /// Descriptor rejected for that role.
        descriptor: String,
    },
}

/// Discovers the Java single abstract method through loaded interface inheritance.
///
/// The walk is deliberately loader-local and bounded. Static, private, default,
/// and public `java.lang.Object` methods do not contribute a SAM candidate.
pub fn discover_functional_interface(
    classes: &BTreeMap<String, Arc<ClassDefinition>>,
    interface: &str,
    node_limit: usize,
) -> Result<FunctionalInterface, FunctionalInterfaceError> {
    let mut pending = vec![interface.to_owned()];
    let mut visited = BTreeSet::new();
    let mut lineage = Vec::new();
    let mut methods: BTreeMap<(String, String), String> = BTreeMap::new();
    while let Some(name) = pending.pop() {
        if visited.contains(&name) {
            continue;
        }
        if visited.len() == node_limit {
            return Err(FunctionalInterfaceError::HierarchyBudgetExhausted { limit: node_limit });
        }
        let class = classes
            .get(&name)
            .ok_or_else(|| FunctionalInterfaceError::MissingClass(name.clone()))?;
        if class.metadata().access_flags() & 0x0200 == 0 {
            return Err(FunctionalInterfaceError::NotInterface(name));
        }
        visited.insert(name.clone());
        lineage.push(name);
        for method in class.metadata().members() {
            if method.kind() != crate::JavaMemberKind::Method
                || method.is_static()
                || !method.is_abstract()
                || method.access_flags() & 0x0002 != 0
                || object_method(method.name(), method.descriptor())
            {
                continue;
            }
            let close = method.descriptor().find(')').ok_or_else(|| {
                FunctionalInterfaceError::InvalidDescriptor(method.descriptor().into())
            })?;
            let key = (
                method.name().to_owned(),
                method.descriptor()[..=close].to_owned(),
            );
            methods
                .entry(key)
                .or_insert_with(|| method.descriptor().to_owned());
        }
        for parent in class.metadata().resolution().direct_parents().iter().rev() {
            if parent != "java.lang.Object" {
                pending.push(parent.clone());
            }
        }
    }
    if methods.is_empty() {
        return Err(FunctionalInterfaceError::NoAbstractMethod {
            interface: interface.into(),
        });
    }
    if methods.len() != 1 {
        return Err(FunctionalInterfaceError::MultipleAbstractMethods {
            methods: methods
                .into_iter()
                .map(|((name, _), descriptor)| format!("{name}{descriptor}"))
                .collect(),
        });
    }
    let ((method_name, _), method_descriptor) = methods.into_iter().next().unwrap();
    Ok(FunctionalInterface {
        interface: interface.into(),
        method_name,
        method_descriptor,
        lineage,
    })
}

/// Validates the located SAM and all metafactory type-bearing inputs.
pub fn validate_functional_interface(
    classes: &BTreeMap<String, Arc<ClassDefinition>>,
    capturing_class: &str,
    invoked_type: &str,
    plan: &LambdaBootstrapPlan,
    implementation_descriptor: &str,
    node_limit: usize,
) -> Result<FunctionalInterface, FunctionalInterfaceError> {
    let (captures, invoked_return) = split_method_descriptor(invoked_type)?;
    let interface = invoked_return
        .strip_prefix('L')
        .and_then(|v| v.strip_suffix(';'))
        .ok_or_else(|| FunctionalInterfaceError::InvalidDescriptor(invoked_type.into()))?
        .replace('/', ".");
    let functional = discover_functional_interface(classes, &interface, node_limit)?;
    if functional.method_descriptor != plan.sam_method_type {
        return Err(FunctionalInterfaceError::SamTypeMismatch {
            discovered: functional.method_descriptor.clone(),
            supplied: plan.sam_method_type.clone(),
        });
    }
    let (sam_args, sam_return) = split_method_descriptor(&plan.sam_method_type)?;
    let (instantiated_args, instantiated_return) =
        split_method_descriptor(&plan.instantiated_method_type)?;
    if sam_args.len() != instantiated_args.len()
        || !types_adapt(&instantiated_args, &sam_args)
        || !return_adapts(&instantiated_return, &sam_return)
    {
        return Err(FunctionalInterfaceError::IncompatibleMethodType {
            role: "instantiated",
            descriptor: plan.instantiated_method_type.clone(),
        });
    }
    let (implementation_args, implementation_return) =
        split_method_descriptor(implementation_descriptor)?;
    let mut supplied = captures;
    supplied.extend(instantiated_args.iter().cloned());
    if !types_adapt(&supplied, &implementation_args)
        || !return_adapts(&implementation_return, &instantiated_return)
    {
        return Err(FunctionalInterfaceError::IncompatibleMethodType {
            role: "implementation",
            descriptor: implementation_descriptor.into(),
        });
    }
    for marker in &plan.marker_interfaces {
        let marker = classes
            .get(marker)
            .ok_or_else(|| FunctionalInterfaceError::MissingClass(marker.clone()))?;
        if marker.metadata().access_flags() & 0x0200 == 0 {
            return Err(FunctionalInterfaceError::NotInterface(
                marker.metadata().resolution().binary_name().into(),
            ));
        }
        let marker_name = marker.metadata().resolution().binary_name();
        if marker.metadata().access_flags() & 0x0001 == 0
            && binary_package(marker_name) != binary_package(capturing_class)
        {
            return Err(FunctionalInterfaceError::InaccessibleInterface(
                marker_name.into(),
            ));
        }
    }
    for bridge in &plan.bridges {
        let (args, result) = split_method_descriptor(bridge)?;
        if args.len() != sam_args.len()
            || !types_adapt(&instantiated_args, &args)
            || !return_adapts(&instantiated_return, &result)
        {
            return Err(FunctionalInterfaceError::IncompatibleMethodType {
                role: "bridge",
                descriptor: bridge.clone(),
            });
        }
    }
    Ok(functional)
}

fn binary_package(name: &str) -> &str {
    name.rsplit_once(['.', '/'])
        .map_or("", |(package, _)| package)
}

fn object_method(name: &str, descriptor: &str) -> bool {
    matches!(
        (name, descriptor),
        ("equals", "(Ljava/lang/Object;)Z")
            | ("hashCode", "()I")
            | ("toString", "()Ljava/lang/String;")
    )
}

fn split_method_descriptor(value: &str) -> Result<(Vec<String>, String), FunctionalInterfaceError> {
    if !valid_method_descriptor(value) {
        return Err(FunctionalInterfaceError::InvalidDescriptor(value.into()));
    }
    let bytes = value.as_bytes();
    let mut cursor = 1;
    let mut args = Vec::new();
    while bytes[cursor] != b')' {
        let start = cursor;
        let _ = parse_descriptor_type(bytes, &mut cursor, false);
        args.push(value[start..cursor].to_owned());
    }
    cursor += 1;
    Ok((args, value[cursor..].to_owned()))
}

fn types_adapt(actual: &[String], expected: &[String]) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(a, e)| a == e || (is_reference(a) && is_reference(e)))
}

fn return_adapts(actual: &str, expected: &str) -> bool {
    expected == "V" || actual == expected || (is_reference(actual) && is_reference(expected))
}

fn is_reference(value: &str) -> bool {
    value.starts_with('L') || value.starts_with('[')
}

/// Decodes and validates a lambda bootstrap before any generated class or instance is allocated.
pub fn decode_lambda_bootstrap(
    owner: &str,
    name: &str,
    descriptor: &str,
    arguments: &[ResolvedBootstrapArgument],
) -> Result<LambdaBootstrapPlan, LambdaBootstrapError> {
    let protocol = LAMBDA_BOOTSTRAP_REGISTRY
        .protocols
        .iter()
        .find(|protocol| {
            protocol.owner == owner && protocol.name == name && protocol.descriptor == descriptor
        })
        .ok_or(LambdaBootstrapError::UnadmittedProtocol)?;
    let method_type = |index: usize| match arguments.get(index) {
        Some(ResolvedBootstrapArgument::MethodType(value)) if valid_method_descriptor(value) => {
            Ok(value.clone())
        }
        _ => Err(LambdaBootstrapError::MalformedPayload(format!(
            "argument {index} must be a valid MethodType"
        ))),
    };
    let sam_method_type = method_type(0)?;
    let implementation_reference_kind = match arguments.get(1) {
        Some(ResolvedBootstrapArgument::MethodHandle { reference_kind }) => {
            if LAMBDA_BOOTSTRAP_REGISTRY
                .reference_kinds
                .contains(&i64::from(*reference_kind))
            {
                *reference_kind
            } else {
                return Err(LambdaBootstrapError::UnadmittedReferenceKind(
                    *reference_kind,
                ));
            }
        }
        _ => {
            return Err(LambdaBootstrapError::MalformedPayload(
                "argument 1 must be a MethodHandle".into(),
            ));
        }
    };
    let instantiated_method_type = method_type(2)?;
    let mut marker_interfaces = Vec::new();
    let mut bridges = Vec::new();
    let mut serializable = false;
    match protocol.tail {
        LambdaProtocolTail::None if arguments.len() != 3 => {
            return Err(LambdaBootstrapError::MalformedPayload(
                "metafactory requires exactly 3 arguments".into(),
            ));
        }
        LambdaProtocolTail::None => {}
        LambdaProtocolTail::FlagGoverned => {
            let flags = match arguments.get(3) {
                Some(ResolvedBootstrapArgument::Integer(flags)) => {
                    let unknown =
                        *flags as u32 & !(LAMBDA_BOOTSTRAP_REGISTRY.admitted_flags_mask as u32);
                    if unknown != 0 {
                        return Err(LambdaBootstrapError::MalformedPayload(format!(
                            "unknown altMetafactory flag bit {}",
                            unknown.trailing_zeros()
                        )));
                    }
                    *flags
                }
                None | Some(_) => {
                    return Err(LambdaBootstrapError::MalformedPayload(
                        "altMetafactory argument 3 must be integer flags".into(),
                    ));
                }
            };
            serializable = flags & 1 != 0;
            let mut cursor = 4;
            if flags & 2 != 0 {
                let count = payload_count(arguments, &mut cursor, "marker")?;
                for _ in 0..count {
                    match arguments.get(cursor) {
                        Some(ResolvedBootstrapArgument::Class(name)) if !name.is_empty() => {
                            if marker_interfaces.iter().any(|marker| marker == name) {
                                return Err(LambdaBootstrapError::MalformedPayload(format!(
                                    "duplicate marker interface {name}"
                                )));
                            }
                            marker_interfaces.push(name.clone())
                        }
                        _ => {
                            return Err(LambdaBootstrapError::MalformedPayload(
                                "marker must be a class".into(),
                            ));
                        }
                    }
                    cursor += 1;
                }
            }
            if flags & 4 != 0 {
                let count = payload_count(arguments, &mut cursor, "bridge")?;
                for _ in 0..count {
                    let bridge = method_type(cursor)?;
                    if bridge == sam_method_type {
                        return Err(LambdaBootstrapError::MalformedPayload(format!(
                            "bridge {bridge} conflicts with the SAM method"
                        )));
                    }
                    if bridges.iter().any(|existing| existing == &bridge) {
                        return Err(LambdaBootstrapError::MalformedPayload(format!(
                            "duplicate bridge {bridge}"
                        )));
                    }
                    bridges.push(bridge);
                    cursor += 1;
                }
            }
            if cursor != arguments.len() {
                return Err(LambdaBootstrapError::MalformedPayload(
                    "trailing altMetafactory arguments".into(),
                ));
            }
        }
    }
    Ok(LambdaBootstrapPlan {
        sam_method_type,
        implementation_reference_kind,
        instantiated_method_type,
        marker_interfaces,
        bridges,
        serializable,
    })
}

fn payload_count(
    arguments: &[ResolvedBootstrapArgument],
    cursor: &mut usize,
    label: &str,
) -> Result<usize, LambdaBootstrapError> {
    let count = match arguments.get(*cursor) {
        Some(ResolvedBootstrapArgument::Integer(value)) if *value >= 0 => {
            usize::try_from(*value).unwrap()
        }
        _ => {
            return Err(LambdaBootstrapError::MalformedPayload(format!(
                "{label} count is missing or negative"
            )));
        }
    };
    *cursor += 1;
    if count > arguments.len().saturating_sub(*cursor) {
        return Err(LambdaBootstrapError::MalformedPayload(format!(
            "{label} payload is truncated"
        )));
    }
    Ok(count)
}

fn valid_method_descriptor(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.first() != Some(&b'(') {
        return false;
    }
    let mut cursor = 1;
    while bytes.get(cursor) != Some(&b')') {
        if !parse_descriptor_type(bytes, &mut cursor, false) {
            return false;
        }
    }
    cursor += 1;
    if bytes.get(cursor) == Some(&b'V') {
        cursor += 1;
    } else if !parse_descriptor_type(bytes, &mut cursor, false) {
        return false;
    }
    cursor == bytes.len()
}

fn parse_descriptor_type(bytes: &[u8], cursor: &mut usize, in_array: bool) -> bool {
    match bytes.get(*cursor).copied() {
        Some(b'B' | b'C' | b'D' | b'F' | b'I' | b'J' | b'S' | b'Z') => {
            *cursor += 1;
            true
        }
        Some(b'L') => {
            *cursor += 1;
            let start = *cursor;
            while !matches!(bytes.get(*cursor), None | Some(b';')) {
                *cursor += 1;
            }
            if *cursor == start || bytes.get(*cursor) != Some(&b';') {
                return false;
            }
            *cursor += 1;
            true
        }
        Some(b'[') if !in_array => {
            while bytes.get(*cursor) == Some(&b'[') {
                *cursor += 1;
            }
            parse_descriptor_type(bytes, cursor, true)
        }
        _ => false,
    }
}
