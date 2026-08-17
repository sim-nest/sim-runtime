/// Resolution-only facts for one ordinary invocation instruction.
#[derive(Clone, Debug)]
pub struct VerificationInvocation<'a> {
    /// Internal name of the symbolic method owner.
    pub owner: &'a str,
    /// Whether the symbolic owner carries `ACC_INTERFACE`.
    pub owner_is_interface: bool,
    /// Resolved declaration; verification never selects or executes its body.
    pub method: &'a JavaMember,
    /// Whether ordinary member-access checks admit the declaration.
    pub accessible: bool,
    /// Whether resolution classified the declaration as signature-polymorphic.
    pub signature_polymorphic: bool,
}

/// Resolution-only facts for one `invokedynamic` instruction.
#[derive(Clone, Debug)]
pub struct VerificationDynamicInvocation<'a> {
    /// Bootstrap identity retained verbatim for fail-closed diagnostics.
    pub bootstrap: &'a DynamicBootstrap,
    /// Invoked method descriptor from the dynamic constant-pool entry.
    pub descriptor: &'a str,
}

/// Applies an ordinary invocation's descriptor and receiver rules without linkage or selection.
pub fn transfer_invocation_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
    invocation: &VerificationInvocation<'_>,
    environment: &VerificationEnvironment<'_>,
    lineage_limit: usize,
) -> Result<VerificationState, VerificationTransferError> {
    use Opcode::*;
    let opcode = instruction.opcode();
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode,
        kind,
    };
    if !matches!(
        opcode,
        Invokevirtual | Invokespecial | Invokestatic | Invokeinterface
    ) || !matches!(
        instruction.instruction().operands.first(),
        Some(InstructionOperand::Constant(_))
    ) {
        return Err(fail(VerificationTransferKind::MalformedPreparedInput));
    }
    if invocation.signature_polymorphic {
        return Err(fail(VerificationTransferKind::SignaturePolymorphic));
    }
    if !invocation.accessible {
        return Err(fail(VerificationTransferKind::MemberAccess));
    }
    let wants_static = opcode == Invokestatic;
    if invocation.method.is_static() != wants_static {
        return Err(fail(VerificationTransferKind::InvocationStaticness));
    }
    if (opcode == Invokeinterface) != invocation.owner_is_interface {
        return Err(fail(VerificationTransferKind::InvocationOwnerKind));
    }
    let (arguments, result) = method_descriptor(invocation.method.descriptor())
        .ok_or_else(|| fail(VerificationTransferKind::InvocationType))?;
    transfer_invocation_values(
        state,
        &arguments,
        result,
        (!wants_static).then_some((invocation.owner, environment, lineage_limit)),
    )
    .map_err(fail)
}

/// Applies an admitted dynamic site's descriptor without consulting or mutating linker state.
pub fn transfer_dynamic_invocation_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
    invocation: &VerificationDynamicInvocation<'_>,
) -> Result<VerificationState, VerificationTransferError> {
    let opcode = instruction.opcode();
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode,
        kind,
    };
    if opcode != Opcode::Invokedynamic
        || !matches!(
            instruction.instruction().operands.first(),
            Some(InstructionOperand::Constant(_))
        )
    {
        return Err(fail(VerificationTransferKind::MalformedPreparedInput));
    }
    let bootstrap = invocation.bootstrap;
    let string_concat = bootstrap.owner == STRING_CONCAT_BOOTSTRAP_OWNER
        && bootstrap.name == STRING_CONCAT_BOOTSTRAP_NAME
        && bootstrap.descriptor == STRING_CONCAT_BOOTSTRAP_DESCRIPTOR;
    let lambda = verifier_admitted_lambda_protocols().iter().any(|protocol| {
        protocol.owner == bootstrap.owner
            && protocol.name == bootstrap.name
            && protocol.descriptor == bootstrap.descriptor
    });
    if !string_concat && !lambda {
        return Err(fail(VerificationTransferKind::DynamicBootstrap(
            DynamicLinkError::UnadmittedBootstrap {
                owner: bootstrap.owner.clone(),
                name: bootstrap.name.clone(),
                descriptor: bootstrap.descriptor.clone(),
            },
        )));
    }
    let (arguments, result) = method_descriptor(invocation.descriptor)
        .ok_or_else(|| fail(VerificationTransferKind::InvocationType))?;
    transfer_invocation_values(state, &arguments, result, None).map_err(fail)
}

/// Returns the verifier's admitted lambda set from the executor-owned registry.
pub fn verifier_admitted_lambda_protocols() -> &'static [crate::LambdaBootstrapProtocol] {
    crate::executor_admitted_lambda_protocols()
}

fn transfer_invocation_values(
    state: &VerificationState,
    arguments: &[VerificationType],
    result: Option<VerificationType>,
    receiver: Option<(&str, &VerificationEnvironment<'_>, usize)>,
) -> Result<VerificationState, VerificationTransferKind> {
    let mut values = stack_values(&state.stack);
    let consumed = arguments.len() + usize::from(receiver.is_some());
    if values.len() < consumed {
        return Err(VerificationTransferKind::StackBounds);
    }
    let base = values.len() - consumed;
    let argument_base = base + usize::from(receiver.is_some());
    if !values[argument_base..]
        .iter()
        .zip(arguments)
        .all(|(actual, expected)| verification_category_matches(actual, expected))
    {
        return Err(VerificationTransferKind::InvocationType);
    }
    if let Some((owner, environment, lineage_limit)) = receiver {
        match &values[base] {
            VerificationType::Null => {}
            VerificationType::Reference(actual)
                if environment
                    .reference_assignability(
                        actual,
                        &ReferenceType::Class(owner.into()),
                        lineage_limit,
                    )
                    .is_ok_and(|answer| answer.value == VerificationAssignability::Assignable) => {}
            _ => return Err(VerificationTransferKind::InvocationType),
        }
    }
    values.truncate(base);
    if let Some(result) = result {
        values.push(result);
    }
    let mut next = state.clone();
    next.stack = stack_from_values(state.stack.capacity(), values)
        .map_err(|_| VerificationTransferKind::StackBounds)?;
    Ok(next)
}

fn method_descriptor(
    descriptor: &str,
) -> Option<(Vec<VerificationType>, Option<VerificationType>)> {
    let arguments = descriptor_arguments(descriptor)?;
    let close = descriptor.find(')')?;
    let result = &descriptor[close + 1..];
    if result == "V" {
        return Some((arguments, None));
    }
    Some((arguments, Some(descriptor_verification_type(result)?)))
}

/// Resolution facts consumed by the object/array/field verifier family.
///
/// These values contain metadata only. Building them through [`VerificationEnvironment`]
/// preserves verification's no-loading and no-initialization boundary.
#[derive(Clone, Debug)]
pub struct VerificationField<'a> {
    /// Binary name of the class that declared the resolved field.
    pub declaring: &'a str,
    /// Resolved field declaration.
    pub field: &'a JavaMember,
    /// Whether JVMS 5.4.4 permits the caller to access the declaration.
    pub accessible: bool,
    /// Whether the caller is a subclass of the declaring class.
    pub caller_is_subclass: bool,
    /// Binary name of the class containing the method being verified.
    pub caller: &'a str,
}

/// Applies fields, arrays, casts, type tests, null checks, and monitor rules.
pub fn transfer_memory_instruction(
    instruction: &crate::PreparedJvmInstruction,
    offset: usize,
    state: &VerificationState,
    field: Option<&VerificationField<'_>>,
) -> Result<VerificationState, VerificationTransferError> {
    use Opcode::*;
    let opcode = instruction.opcode();
    let fail = |kind| VerificationTransferError {
        instruction: instruction.id(),
        offset,
        opcode,
        kind,
    };
    let mut values = stack_values(&state.stack);
    let pop = |values: &mut Vec<VerificationType>| {
        values
            .pop()
            .ok_or_else(|| fail(VerificationTransferKind::StackBounds))
    };
    let reference = |value: &VerificationType| {
        matches!(
            value,
            VerificationType::Null | VerificationType::Reference(_)
        )
    };
    match opcode {
        Getstatic | Putstatic | Getfield | Putfield => {
            let resolved =
                field.ok_or_else(|| fail(VerificationTransferKind::MalformedPreparedInput))?;
            let wants_static = matches!(opcode, Getstatic | Putstatic);
            if resolved.field.is_static() != wants_static {
                return Err(fail(VerificationTransferKind::FieldStaticness));
            }
            if !resolved.accessible {
                return Err(fail(VerificationTransferKind::MemberAccess));
            }
            let ty = descriptor_verification_type(resolved.field.descriptor())
                .ok_or_else(|| fail(VerificationTransferKind::MemoryType))?;
            if matches!(opcode, Putstatic | Putfield) {
                let actual = pop(&mut values)?;
                if !verification_category_matches(&actual, &ty) {
                    return Err(fail(VerificationTransferKind::MemoryType));
                }
            }
            if matches!(opcode, Getfield | Putfield) {
                let receiver = pop(&mut values)?;
                if !reference(&receiver) {
                    return Err(fail(VerificationTransferKind::MemoryType));
                }
                if resolved.field.access_flags() & 0x0004 != 0
                    && resolved.caller_is_subclass
                    && resolved.caller != resolved.declaring
                    && !matches!(&receiver, VerificationType::Null)
                    && !matches!(&receiver, VerificationType::Reference(ReferenceType::Class(name)) if name.as_ref() == resolved.caller)
                {
                    return Err(fail(VerificationTransferKind::ProtectedMemberAccess));
                }
            }
            if matches!(opcode, Getstatic | Getfield) {
                values.push(ty);
            }
        }
        Aaload => {
            if pop(&mut values)? != VerificationType::Int {
                return Err(fail(VerificationTransferKind::MemoryType));
            }
            let receiver = pop(&mut values)?;
            let component = array_component(&receiver)
                .ok_or_else(|| fail(VerificationTransferKind::ArrayType))?;
            if component.is_empty() {
                values.push(VerificationType::Reference(ReferenceType::Object));
            } else if is_primitive_descriptor(component) {
                return Err(fail(VerificationTransferKind::ArrayType));
            } else {
                values.push(
                    descriptor_reference(component)
                        .map(VerificationType::Reference)
                        .map_err(|_| fail(VerificationTransferKind::ArrayType))?,
                );
            }
        }
        Arraylength => {
            if array_component(&pop(&mut values)?).is_none() {
                return Err(fail(VerificationTransferKind::ArrayType));
            }
            values.push(VerificationType::Int);
        }
        Checkcast | Instanceof => {
            if !reference(&pop(&mut values)?) {
                return Err(fail(VerificationTransferKind::MemoryType));
            }
            values.push(if opcode == Instanceof {
                VerificationType::Int
            } else {
                VerificationType::Reference(ReferenceType::Object)
            });
        }
        Ifnull | Ifnonnull | Monitorenter | Monitorexit => {
            if !reference(&pop(&mut values)?) {
                return Err(fail(VerificationTransferKind::MemoryType));
            }
        }
        Newarray | Anewarray | Multianewarray => {
            let dimensions = 1;
            for _ in 0..dimensions {
                if pop(&mut values)? != VerificationType::Int {
                    return Err(fail(VerificationTransferKind::MemoryType));
                }
            }
            values.push(VerificationType::Reference(ReferenceType::Array(
                "[Ljava/lang/Object;".into(),
            )));
        }
        _ => return Err(fail(VerificationTransferKind::MalformedPreparedInput)),
    }
    let mut next = state.clone();
    next.stack = stack_from_values(state.stack.capacity(), values)
        .map_err(|_| fail(VerificationTransferKind::StackBounds))?;
    Ok(next)
}

fn descriptor_verification_type(descriptor: &str) -> Option<VerificationType> {
    let mut cursor = 0;
    let ty = parse_descriptor_type(descriptor, &mut cursor).ok()?;
    (cursor == descriptor.len()).then_some(ty)
}

fn array_component(value: &VerificationType) -> Option<&str> {
    match value {
        VerificationType::Null => Some(""),
        VerificationType::Reference(ReferenceType::Array(descriptor)) => {
            descriptor.strip_prefix('[')
        }
        _ => None,
    }
}
