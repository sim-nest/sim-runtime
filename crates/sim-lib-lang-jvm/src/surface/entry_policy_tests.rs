mod entry_policy_tests {
    use super::*;
    use sim_kernel::{DefaultFactory, EagerPolicy};

    fn context() -> Cx {
        let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
        seat.grant(&mut cx, crate::class_load_capability()).unwrap();
        seat.grant(&mut cx, jvm_invoke_capability()).unwrap();
        cx
    }

    #[test]
    fn verified_required_live_entry_accepts_only_the_exact_proof() {
        let mut cx = context();
        let surface = JvmSurface::new(65_536);
        let class = surface.define(&mut cx, "StaticInt", include_bytes!("../../fixtures/javac/StaticInt.class").to_vec()).unwrap();
        let policy = ValueFingerprint::new(17);
        let structural = ValueFingerprint::new(29);
        let methods = class.metadata().members().iter()
            .filter(|member| member.kind() == crate::JavaMemberKind::Method && !member.is_abstract())
            .map(|member| format!("{}{}", member.name(), member.descriptor())).collect::<Vec<_>>();
        let names = methods.iter().map(String::as_str).collect::<Vec<_>>();
        let proof = ClassVerificationProof::test(class.id().clone(), surface.loader.revision(), policy, structural, &names);
        let verified = JvmEntryPolicy::Verified { proof: &proof, policy, structural, frames: &[] };
        assert_eq!(surface.invoke_static_i32_with_policy(&mut cx, "StaticInt", "wholePipeline", "(II)I", &[3, 4], verified).unwrap(), 14);
        assert_eq!(surface.last_drive_receipts().unwrap().0.fidelity, crate::VerificationFidelity::Verified);
        let stale = JvmEntryPolicy::Verified { proof: &proof, policy: ValueFingerprint::new(18), structural, frames: &[] };
        assert!(matches!(surface.invoke_static_i32_with_policy(&mut cx, "StaticInt", "wholePipeline", "(II)I", &[3, 4], stale), Err(JvmInvocationError::Admission(_))));
    }
}
