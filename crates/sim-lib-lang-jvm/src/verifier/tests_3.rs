    fn types() -> Vec<VerificationType> {
        vec![
            VerificationType::Bottom,
            VerificationType::Int,
            VerificationType::Float,
            VerificationType::Long,
            VerificationType::Double,
            VerificationType::Null,
            VerificationType::Reference(ReferenceType::Object),
            VerificationType::Reference(ReferenceType::Class("java/lang/String".into())),
            VerificationType::Reference(ReferenceType::Array("[I".into())),
            VerificationType::UninitializedThis,
            VerificationType::Uninitialized(7),
            VerificationType::Uninitialized(11),
            VerificationType::Unusable,
        ]
    }

    #[test]
    fn every_verification_type_pair_and_triple_obeys_the_delivered_laws() {
        LawSuite::check_lattice(&types()).unwrap();
    }

    #[test]
    fn exhaustive_small_frames_obey_the_delivered_laws() {
        let values = types()
            .into_iter()
            .filter(|value| value.width().is_some())
            .collect::<Vec<_>>();
        let mut frames = vec![
            VerificationFrame::bottom_frame(FrameKind::Locals, 2),
            VerificationFrame::new(FrameKind::Locals, 2),
        ];
        for first in &values {
            let mut frame = VerificationFrame::new(FrameKind::Locals, 2);
            if frame.set_local(0, first.clone()).is_ok() {
                frames.push(frame);
            }
            for second in &values {
                let mut frame = VerificationFrame::new(FrameKind::Locals, 2);
                if frame.set_local(0, first.clone()).is_ok()
                    && frame.set_local(1, second.clone()).is_ok()
                {
                    frames.push(frame);
                }
            }
        }
        LawSuite::check_lattice(&frames).unwrap();
    }

    #[test]
    fn half_overwriting_category_two_local_makes_the_old_value_unusable() {
        let mut locals = VerificationFrame::new(FrameKind::Locals, 3);
        locals.set_local(0, VerificationType::Long).unwrap();
        locals.set_local(1, VerificationType::Int).unwrap();
        assert_eq!(locals.get(0), None);
        assert_eq!(locals.get(1), Some(&VerificationType::Int));
    }

    #[test]
    fn operand_frames_charge_category_widths() {
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 3);
        stack.push(VerificationType::Long).unwrap();
        stack.push(VerificationType::Int).unwrap();
        assert_eq!(stack.get(0), Some(&VerificationType::Long));
        assert_eq!(stack.get(2), Some(&VerificationType::Int));
    }

    #[test]
    fn conditional_branches_pop_once_for_both_successors_and_returns_match_the_method() {
        let branch = prepared(
            &[
                Opcode::Iconst0 as u8,
                Opcode::Ifeq as u8,
                0,
                4,
                Opcode::Return as u8,
                Opcode::Return as u8,
            ],
            &[],
        );
        let instruction = branch.instruction(branch.next(branch.entry()).unwrap());
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 1);
        stack.push(VerificationType::Int).unwrap();
        let state = VerificationState {
            locals: VerificationFrame::new(FrameKind::Locals, 0),
            stack,
        };
        let next = transfer_control_instruction(
            instruction.instruction(),
            1,
            &state,
            &VerificationReturnType::Void,
        )
        .unwrap();
        assert!(stack_values(&next.stack).is_empty());

        let returning = prepared(&[Opcode::Ireturn as u8], &[]);
        transfer_control_instruction(
            returning.instruction(returning.entry()).instruction(),
            0,
            &state,
            &VerificationReturnType::Value(VerificationType::Int),
        )
        .unwrap();
        let error = transfer_control_instruction(
            returning.instruction(returning.entry()).instruction(),
            0,
            &state,
            &VerificationReturnType::Void,
        )
        .unwrap_err();
        assert_eq!(error.kind, VerificationTransferKind::ReturnType);
    }

    #[test]
    fn joined_targets_require_assignable_declared_frames_for_modern_classfiles() {
        let code = prepared(
            &[
                Opcode::Iconst0 as u8,
                Opcode::Ifeq as u8,
                0,
                4,
                Opcode::Return as u8,
                Opcode::Return as u8,
            ],
            &[],
        );
        let graph = build_verification_graph(&code).unwrap();
        let target = InstructionId(3);
        let state = VerificationState {
            locals: VerificationFrame::new(FrameKind::Locals, 0),
            stack: VerificationFrame::new(FrameKind::OperandStack, 1),
        };
        let inferred = BTreeMap::from([(target, state)]);
        let correct = ExpandedStackMapFrame {
            offset: 5,
            instruction: target,
            locals: Box::new([]),
            stack: Box::new([]),
        };
        check_stack_map_constraints(61, &graph, &inferred, std::slice::from_ref(&correct), 0, 1)
            .unwrap();

        let wider = ExpandedStackMapFrame {
            stack: Box::new([VerificationType::Int]),
            ..correct
        };
        assert_eq!(
            check_stack_map_constraints(61, &graph, &inferred, &[wider], 0, 1),
            Err(StackMapConstraintError::NotAssignable {
                instruction: target
            })
        );
        assert_eq!(
            check_stack_map_constraints(61, &graph, &inferred, &[], 0, 1),
            Err(StackMapConstraintError::Missing {
                instruction: target
            })
        );
    }
