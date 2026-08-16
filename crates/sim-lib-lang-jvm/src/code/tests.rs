#[cfg(test)]
mod identity_tests {
    use std::sync::Arc;

    use sim_incremental_core::ValueFingerprint;
    use sim_kernel::{Cx, DefaultFactory, EagerPolicy};

    use super::{PreparedCodeIdentity, PreparedMicroOp, VerificationPreparation, select_micro_op};
    use crate::{
        ClassLoader, ClassVerificationProof, FrameKind, VerificationFrame, VerificationState,
        VerificationType, class_load_capability,
    };

    #[test]
    fn class_space_revision_bump_invalidates_prepared_code_identity() {
        let loader = ClassLoader::new(16);
        let bytes = [0x03, 0xac];
        let identity = PreparedCodeIdentity::new(&bytes, loader.revision());
        assert!(identity.matches(&bytes, loader.revision()));
        loader.simulate_class_space_change();
        assert!(!identity.matches(&bytes, loader.revision()));
        assert!(!identity.matches(&[0x04, 0xac], identity.revision()));
    }

    fn exact_fixture() -> (
        ClassLoader,
        Arc<crate::ClassDefinition>,
        ClassVerificationProof,
        VerificationState,
    ) {
        let loader = ClassLoader::new(4096);
        let (mut cx, seat) = Cx::new_seated(Arc::new(EagerPolicy), Arc::new(DefaultFactory));
        seat.grant(&mut cx, class_load_capability()).unwrap();
        let definition = loader
            .define_bytes(
                &mut cx,
                "Minimal",
                include_bytes!("../../fixtures/hand-built/Minimal.class").to_vec(),
            )
            .unwrap();
        let proof = ClassVerificationProof::test(
            definition.id().clone(),
            loader.revision(),
            ValueFingerprint::new(7),
            ValueFingerprint::new(8),
            &["value()I"],
        );
        let mut locals = VerificationFrame::new(FrameKind::Locals, 2);
        locals.set_local(0, VerificationType::Int).unwrap();
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 2);
        stack.push(VerificationType::Long).unwrap();
        (
            loader,
            definition,
            proof,
            VerificationState { locals, stack },
        )
    }

    #[test]
    fn verified_micro_op_requires_every_exact_identity_component() {
        let (loader, definition, proof, state) = exact_fixture();
        let frames = [(sim_codec_classfile::InstructionId(0), state)];
        let exact = VerificationPreparation {
            proof: &proof,
            owner: definition.id(),
            revision: loader.revision(),
            policy: ValueFingerprint::new(7),
            structural: ValueFingerprint::new(8),
            method: "value()I",
            method_proof: ValueFingerprint::new(1),
            frames: &frames,
        };
        let selected = select_micro_op(
            Some(&exact),
            sim_codec_classfile::InstructionId(0),
            &[],
            &[],
        );
        let PreparedMicroOp::Verified(guarantee) = selected else {
            panic!("exact proof refused")
        };
        assert_eq!(guarantee.stack_width(), 2);
        assert_eq!(guarantee.local_width(), 2);

        for mutation in 1..5 {
            let candidate = VerificationPreparation {
                revision: if mutation == 1 {
                    loader.simulate_class_space_change();
                    loader.revision()
                } else {
                    proof.owner_revision()
                },
                method: if mutation == 2 {
                    "wrong()V"
                } else {
                    "value()I"
                },
                policy: if mutation == 3 {
                    ValueFingerprint::new(70)
                } else {
                    ValueFingerprint::new(7)
                },
                frames: if mutation == 4 { &[] } else { &frames },
                ..exact
            };
            assert_eq!(
                select_micro_op(
                    Some(&candidate),
                    sim_codec_classfile::InstructionId(0),
                    &[],
                    &[]
                ),
                PreparedMicroOp::Checked
            );
        }
    }
}
