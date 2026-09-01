#[allow(
    clippy::too_many_arguments,
    reason = "the typed request keeps each independently admitted method input explicit"
)]
fn execute_prepared_i32(
    loader: &ClassLoader,
    heap: &Mutex<crate::JvmHeap>,
    frames: &crate::JvmFramePool,
    cache: &Mutex<BTreeMap<String, Arc<PreparedSurfaceMethod>>>,
    decode_count: &AtomicUsize,
    class: &Arc<ClassDefinition>,
    name: &str,
    descriptor: &AdmittedI32Descriptor,
    args: &[i32],
    policy: JvmEntryPolicy<'_>,
) -> std::result::Result<
    (i32, crate::JvmPreparationReceipt, crate::JvmDriveReceipt),
    JvmInvocationError,
> {
    let policy_key = match &policy {
        JvmEntryPolicy::StaticChecked => "static".into(),
        JvmEntryPolicy::Verified { proof, .. } => format!("verified:{:?}", proof.identity()),
    };
    let cache_key = format!(
        "{:?}:{policy_key}:{}.{}{}",
        loader.revision(),
        class.id().binary_name(),
        name,
        descriptor.text
    );
    let cached = cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&cache_key)
        .cloned();
    let prepared_method = if let Some(cached) = cached {
        cached
    } else {
        let index = class
            .metadata()
            .members()
            .iter()
            .filter(|m| matches!(m.kind(), crate::JavaMemberKind::Method))
            .position(|m| m.name() == name && m.descriptor() == descriptor.text)
            .ok_or_else(|| Error::Eval("selected JVM method body is missing".into()))?;
        let method = class
            .shell()
            .methods
            .get(index)
            .ok_or_else(|| Error::Eval("selected JVM method shell is missing".into()))?;
        let code = code_attribute(class, method)?
            .ok_or_else(|| Error::Eval("selected JVM method has no Code attribute".into()))?;
        let decoded = decode_instructions(
            &code.code,
            class.shell().major_version,
            &class.shell().constant_pool,
        )
        .map_err(|e| Error::Eval(e.to_string()))?;
        decode_count.fetch_add(1, Ordering::Relaxed);
        let source = sim_kernel::SourceId(format!(
            "{}.{}{}",
            class.id().binary_name(),
            name,
            descriptor.text
        ));
        let method_identity = format!("{}{}", name, descriptor.text);
        let prepared = match &policy {
            JvmEntryPolicy::StaticChecked => {
                crate::prepare_code_bound::<crate::driver::SurfacePolicy>(
                    &decoded,
                    &code.code,
                    &code.exception_table,
                    source,
                    loader.revision(),
                )
            }
            JvmEntryPolicy::Verified {
                proof,
                policy,
                structural,
                frames,
            } => {
                let method_proof = proof
                    .methods()
                    .iter()
                    .find(|candidate| candidate.method() == method_identity)
                    .ok_or_else(|| {
                        invocation_admission("verification proof does not cover selected method")
                    })?;
                crate::prepare_code_verified::<crate::driver::SurfacePolicy>(
                    &decoded,
                    &code.code,
                    &code.exception_table,
                    source,
                    crate::VerificationPreparation {
                        proof,
                        owner: class.id(),
                        revision: loader.revision(),
                        policy: *policy,
                        structural: *structural,
                        method: &method_identity,
                        method_proof: method_proof.proof(),
                        frames,
                    },
                )
            }
        }
        .map_err(|e| invocation_admission(format!("method preparation: {e:?}")))?;
        let limits = sim_lib_machine::AdmissionLimits {
            instructions: prepared.len(),
            operand_units: usize::from(code.max_stack).max(1),
            slots: usize::from(code.max_locals).max(1),
            frames: 1,
            work: 65_536,
        };
        let description = sim_lib_machine::MachineDescription::new(&prepared, limits, &());
        let machine =
            sim_lib_machine::MachinePermit::admit::<_, _, crate::driver::SurfaceAdmission>(
                &description,
            )
            .map_err(|e| invocation_admission(format!("machine admission: {e:?}")))?;
        let prepared_method = Arc::new(PreparedSurfaceMethod {
            code: prepared,
            machine,
            limits,
            max_locals: usize::from(code.max_locals),
            max_stack: usize::from(code.max_stack),
        });
        cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(cache_key, prepared_method.clone());
        prepared_method
    };
    let target = crate::EntryTarget::Method {
        name: name.into(),
        descriptor: descriptor.text.clone(),
    };
    let admitted = crate::ClassfilePermit::new(loader, class.clone())
        .map_err(invocation_admission)?
        .resolve(target)
        .map_err(invocation_admission)?
        .admit(&prepared_method.machine);
    let mut lease = frames.acquire(prepared_method.max_locals, prepared_method.max_stack);
    for (slot, value) in args.iter().copied().enumerate() {
        lease
            .frame_mut()
            .locals_mut()
            .store(slot, crate::JvmValue::Int(value))
            .map_err(|error| invocation_resource(format!("local initialization: {error:?}")))?;
    }
    macro_rules! drive {
        ($permit:expr) => {
            crate::driver::drive_i32(
                $permit,
                &prepared_method.code,
                &prepared_method.machine,
                prepared_method.limits,
                lease,
                heap,
            )
        };
    }
    match policy {
        JvmEntryPolicy::StaticChecked => drive!(
            admitted
                .verify(&crate::NoVerifier)
                .map_err(invocation_admission)?
                .permit()
        ),
        JvmEntryPolicy::Verified {
            proof,
            policy,
            structural,
            ..
        } => drive!(
            admitted
                .verify(&crate::ClassVerifierProvider::exact(
                    Arc::new(proof.clone()),
                    policy,
                    structural
                ))
                .map_err(invocation_admission)?
                .permit()
        ),
    }
}
