    fn frame_input<'a>(
        name: &'a str,
        descriptor: &'a str,
        is_static: bool,
    ) -> InitialFrameInput<'a> {
        InitialFrameInput {
            declaring_class: "sample/Owner",
            method_name: name,
            descriptor,
            is_static,
            max_locals: 8,
            max_stack: 4,
        }
}
    #[test]
    fn initial_locals_are_exact_for_static_instance_and_constructor_descriptors() {
        assert_eq!(
            derive_initial_locals(&frame_input("work", "(IJ[Ljava/lang/String;)V", true)).unwrap(),
            [
                VerificationType::Int,
                VerificationType::Long,
                VerificationType::Reference(ReferenceType::Array("[Ljava/lang/String;".into())),
            ]
        );
        assert_eq!(
            derive_initial_locals(&frame_input("work", "(D)Ljava/lang/Object;", false)).unwrap(),
            [
                VerificationType::Reference(ReferenceType::Class("sample/Owner".into())),
                VerificationType::Double,
            ]
        );
        assert_eq!(
            derive_initial_locals(&frame_input("<init>", "()V", false)).unwrap(),
            [VerificationType::UninitializedThis]
        );
    }

    fn storage_transfer(
        bytes: &[u8],
        locals: VerificationFrame,
        stack_values: &[VerificationType],
        stack_capacity: usize,
    ) -> Result<VerificationState, VerificationTransferError> {
        let pool = if matches!(bytes.first(), Some(opcode) if *opcode == Opcode::Ldc as u8) {
            ConstantPool::decode(&mut ByteReader::new(&[0, 2, 3, 0, 0, 0, 42], 7), 61).unwrap()
        } else {
            empty_pool()
        };
        let decoded = decode_instructions(bytes, 61, &pool).unwrap();
        let code = prepare_code::<GraphPolicy>(
            &decoded,
            bytes.len(),
            &[],
            SourceId("Verifier.storage()V".into()),
        )
        .unwrap();
        let instruction = code.instruction(code.entry());
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, stack_capacity);
        for value in stack_values {
            stack.push(value.clone()).unwrap();
        }
        transfer_storage_instruction(
            instruction.instruction(),
            0,
            &VerificationState { locals, stack },
            &|index| (index == 1).then_some(VerificationType::Int),
        )
    }

    fn memory_transfer(
        opcode: Opcode,
        input: Vec<VerificationType>,
        field: Option<&VerificationField<'_>>,
    ) -> Result<VerificationState, VerificationTransferError> {
        let bytes = if matches!(
            opcode,
            Opcode::Getstatic
                | Opcode::Putstatic
                | Opcode::Getfield
                | Opcode::Putfield
                | Opcode::Checkcast
                | Opcode::Instanceof
                | Opcode::Anewarray
        ) {
            vec![opcode as u8, 0, 6]
        } else {
            vec![opcode as u8]
        };
        let pool = if matches!(
            opcode,
            Opcode::Getstatic | Opcode::Putstatic | Opcode::Getfield | Opcode::Putfield
        ) {
            field_pool()
        } else {
            empty_pool()
        };
        let decoded = decode_instructions(&bytes, 61, &pool).unwrap();
        let code = prepare_code::<GraphPolicy>(
            &decoded,
            bytes.len(),
            &[],
            SourceId("Verifier.memory()V".into()),
        )
        .unwrap();
        let instruction = code.instruction(code.entry()).instruction();
        let state = VerificationState {
            locals: VerificationFrame::new(FrameKind::Locals, 0),
            stack: stack_from_values(8, input).unwrap(),
        };
        transfer_memory_instruction(instruction, 0, &state, field)
    }

    #[test]
    fn aaload_refuses_a_primitive_array_under_jvms_4_10_1_9() {
        let error = memory_transfer(
            Opcode::Aaload,
            vec![
                VerificationType::Reference(ReferenceType::Array("[I".into())),
                VerificationType::Int,
            ],
            None,
        )
        .unwrap_err();
        assert_eq!(error.kind, VerificationTransferKind::ArrayType);
    }

    #[test]
    fn protected_field_receiver_obeys_jvms_4_10_1_8() {
        let declaration = JavaMember::test_field("value", "I", 0x0004);
        let field = VerificationField {
            declaring: "base.Owner",
            field: &declaration,
            accessible: true,
            caller_is_subclass: true,
            caller: "other.Child",
        };
        let error = memory_transfer(
            Opcode::Getfield,
            vec![VerificationType::Reference(ReferenceType::Class(
                "unrelated.Peer".into(),
            ))],
            Some(&field),
        )
        .unwrap_err();
        assert_eq!(error.kind, VerificationTransferKind::ProtectedMemberAccess);
    }

    #[test]
    fn constants_locals_stores_and_iinc_preserve_typed_bounds() {
        let input = frame_input("work", "(I)V", true);
        let initial = VerificationState::initial(&input).unwrap();
        assert_eq!(initial.locals.get(0), Some(&VerificationType::Int));

        let loaded =
            storage_transfer(&[Opcode::Iload0 as u8], initial.locals.clone(), &[], 2).unwrap();
        assert_eq!(stack_values(&loaded.stack), [VerificationType::Int]);
        let incremented =
            storage_transfer(&[Opcode::Iinc as u8, 0, 7], loaded.locals.clone(), &[], 2).unwrap();
        assert_eq!(incremented.locals.get(0), Some(&VerificationType::Int));

        let stored = storage_transfer(
            &[Opcode::Lstore as u8, 6],
            VerificationFrame::new(FrameKind::Locals, 8),
            &[VerificationType::Long],
            2,
        )
        .unwrap();
        assert_eq!(stored.locals.get(6), Some(&VerificationType::Long));
        let error = storage_transfer(
            &[Opcode::Lstore as u8, 7],
            VerificationFrame::new(FrameKind::Locals, 8),
            &[VerificationType::Long],
            2,
        )
        .unwrap_err();
        assert_eq!(error.instruction, InstructionId(0));
        assert_eq!(error.offset, 0);
        assert_eq!(error.kind, VerificationTransferKind::LocalBounds);

        let pushed = storage_transfer(
            &[Opcode::Ldc as u8, 1],
            VerificationFrame::new(FrameKind::Locals, 0),
            &[],
            1,
        )
        .unwrap();
        assert_eq!(stack_values(&pushed.stack), [VerificationType::Int]);
    }

    #[test]
    fn every_shuffle_form_uses_the_executor_descriptor() {
        use VerificationType::{Double, Float, Int, Long};
        let cases: &[(&[u8], &[VerificationType], &[VerificationType])] = &[
            (&[Opcode::Pop as u8], &[Int], &[]),
            (&[Opcode::Pop2 as u8], &[Long], &[]),
            (&[Opcode::Dup as u8], &[Int], &[Int, Int]),
            (&[Opcode::DupX1 as u8], &[Int, Float], &[Float, Int, Float]),
            (&[Opcode::DupX2 as u8], &[Long, Int], &[Int, Long, Int]),
            (&[Opcode::Dup2 as u8], &[Long], &[Long, Long]),
            (&[Opcode::Dup2X1 as u8], &[Int, Long], &[Long, Int, Long]),
            (
                &[Opcode::Dup2X2 as u8],
                &[Double, Long],
                &[Long, Double, Long],
            ),
            (&[Opcode::Swap as u8], &[Int, Float], &[Float, Int]),
        ];
        for (bytes, input, expected) in cases {
            let state = storage_transfer(
                bytes,
                VerificationFrame::new(FrameKind::Locals, 0),
                input,
                8,
            )
            .unwrap();
            assert_eq!(stack_values(&state.stack), *expected);
        }
    }

    fn numeric_transfer(
        opcode: Opcode,
        input: &[VerificationType],
    ) -> Result<VerificationState, VerificationTransferError> {
        let decoded = decode_instructions(&[opcode as u8], 61, &empty_pool()).unwrap();
        let code =
            prepare_code::<GraphPolicy>(&decoded, 1, &[], SourceId("Verifier.numeric()V".into()))
                .unwrap();
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 8);
        stack.push(VerificationType::Float).unwrap();
        for value in input {
            stack.push(value.clone()).unwrap();
        }
        transfer_numeric_instruction(
            code.instruction(code.entry()).instruction(),
            0,
            &VerificationState {
                locals: VerificationFrame::new(FrameKind::Locals, 0),
                stack,
            },
        )
    }

    #[test]
    fn every_numeric_opcode_has_exact_passing_and_failing_frames() {
        use VerificationType::{Double, Float, Int, Long};
        let mut covered = Vec::new();
        for rule in VERIFIER_RULES
            .iter()
            .filter(|rule| rule.family == VerifierRuleFamily::NumericConversion)
        {
            if rule.opcode == Opcode::Iinc {
                continue;
            }
            let (input, output) = numeric_signature(rule.opcode)
                .unwrap_or_else(|| panic!("missing numeric signature for {:?}", rule.opcode));
            let passed = numeric_transfer(rule.opcode, input).unwrap();
            assert_eq!(stack_values(&passed.stack), [Float, output.clone()]);

            let mut wrong = input.to_vec();
            let last = wrong
                .last_mut()
                .expect("every numeric rule consumes a value");
            *last = match last {
                Int => Long,
                Long | Float | Double => Int,
                other => panic!("unexpected numeric input {other:?}"),
            };
            let error = numeric_transfer(rule.opcode, &wrong).unwrap_err();
            assert_eq!(error.opcode, rule.opcode);
            assert_eq!(error.kind, VerificationTransferKind::Category);
            covered.push(rule.opcode);
        }
        assert_eq!(covered.len(), 56);

        let mut locals = VerificationFrame::new(FrameKind::Locals, 1);
        locals.set_local(0, Int).unwrap();
        let decoded = decode_instructions(&[Opcode::Iinc as u8, 0, 1], 61, &empty_pool()).unwrap();
        let code =
            prepare_code::<GraphPolicy>(&decoded, 3, &[], SourceId("Verifier.iinc()V".into()))
                .unwrap();
        let instruction = code.instruction(code.entry()).instruction();
        let state = VerificationState {
            locals: locals.clone(),
            stack: VerificationFrame::new(FrameKind::OperandStack, 0),
        };
        assert_eq!(
            transfer_numeric_instruction(instruction, 0, &state).unwrap(),
            state
        );
        locals.set_local(0, Float).unwrap();
        let error = transfer_numeric_instruction(
            instruction,
            0,
            &VerificationState {
                locals,
                stack: state.stack,
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, VerificationTransferKind::Category);
    }

    #[test]
    fn long_shift_requires_an_int_count_and_preserves_category_two_layout() {
        let shifted = numeric_transfer(
            Opcode::Lshl,
            &[VerificationType::Long, VerificationType::Int],
        )
        .unwrap();
        assert_eq!(
            shifted.stack.normalized_slots().unwrap(),
            &[
                Slot::Value(VerificationType::Float),
                Slot::Value(VerificationType::Long),
                Slot::Category2Tail,
                Slot::Unusable,
                Slot::Unusable,
                Slot::Unusable,
                Slot::Unusable,
                Slot::Unusable,
            ]
        );
        assert_eq!(
            numeric_transfer(
                Opcode::Lshl,
                &[VerificationType::Long, VerificationType::Long]
            )
            .unwrap_err()
            .kind,
            VerificationTransferKind::Category
        );
    }

    #[test]
    fn dup_x1_rejects_a_category_two_split_at_the_instruction_origin() {
        let error = storage_transfer(
            &[Opcode::DupX1 as u8],
            VerificationFrame::new(FrameKind::Locals, 0),
            &[VerificationType::Long, VerificationType::Int],
            4,
        )
        .unwrap_err();
        assert_eq!(error.instruction, InstructionId(0));
        assert_eq!(error.offset, 0);
        assert_eq!(error.opcode, Opcode::DupX1);
        assert_eq!(error.kind, VerificationTransferKind::Category);
    }

    #[test]
    fn every_compressed_frame_form_expands_to_independent_expected_state() {
        use ClassfileVerificationType as C;
        let code = prepared(&[Opcode::Return as u8; 7], &[]);
        let table = StackMapTableAttribute {
            frames: vec![
                StackMapFrame::Same { frame_type: 0 },
                StackMapFrame::SameLocalsOneStack {
                    frame_type: 64,
                    stack: C::Integer,
                },
                StackMapFrame::SameLocalsOneStackExtended {
                    offset_delta: 0,
                    stack: C::Long,
                },
                StackMapFrame::Append {
                    frame_type: 252,
                    offset_delta: 0,
                    locals: vec![C::Float],
                },
                StackMapFrame::Chop {
                    frame_type: 250,
                    offset_delta: 0,
                },
                StackMapFrame::SameExtended { offset_delta: 0 },
                StackMapFrame::Full {
                    offset_delta: 0,
                    locals: vec![C::Object(7), C::Double],
                    stack: vec![C::Null, C::Uninitialized(3)],
                },
            ],
        };
        let actual = expand_stack_map_table(
            &table,
            &frame_input("work", "(J)V", false),
            &code,
            |index| (index == 7).then(|| ReferenceType::Class("java/lang/Object".into())),
        )
        .unwrap();
        let owner = VerificationType::Reference(ReferenceType::Class("sample/Owner".into()));
        let expectations = vec![
            (vec![owner.clone(), VerificationType::Long], vec![]),
            (
                vec![owner.clone(), VerificationType::Long],
                vec![VerificationType::Int],
            ),
            (
                vec![owner.clone(), VerificationType::Long],
                vec![VerificationType::Long],
            ),
            (
                vec![
                    owner.clone(),
                    VerificationType::Long,
                    VerificationType::Float,
                ],
                vec![],
            ),
            (vec![owner.clone(), VerificationType::Long], vec![]),
            (vec![owner, VerificationType::Long], vec![]),
            (
                vec![
                    VerificationType::Reference(ReferenceType::Class("java/lang/Object".into())),
                    VerificationType::Double,
                ],
                vec![VerificationType::Null, VerificationType::Uninitialized(3)],
            ),
        ];
        for (index, (frame, (locals, stack))) in actual.iter().zip(expectations).enumerate() {
            assert_eq!(frame.offset, index as u32);
            assert_eq!(frame.instruction, InstructionId(index as u32));
            assert_eq!(frame.locals.as_ref(), locals);
            assert_eq!(frame.stack.as_ref(), stack);
        }
    }

    #[test]
    fn non_boundary_stack_map_offset_is_rejected_naming_the_offset() {
        let code = prepared(&[Opcode::Goto as u8, 0, 3, Opcode::Return as u8], &[]);
        let error = expand_stack_map_table(
            &StackMapTableAttribute {
                frames: vec![StackMapFrame::Same { frame_type: 1 }],
            },
            &frame_input("work", "()V", true),
            &code,
            |_| None,
        )
        .unwrap_err();
        assert_eq!(
            error,
            StackMapExpansionError::NotInstructionBoundary { offset: 1 }
        );
    }

    #[test]
    fn expanded_frames_enforce_physical_local_and_stack_widths() {
        let code = prepared(&[Opcode::Return as u8], &[]);
        let mut input = frame_input("work", "()V", true);
        input.max_locals = 1;
        input.max_stack = 1;
        let locals_error = expand_stack_map_table(
            &StackMapTableAttribute {
                frames: vec![StackMapFrame::Full {
                    offset_delta: 0,
                    locals: vec![ClassfileVerificationType::Long],
                    stack: vec![],
                }],
            },
            &input,
            &code,
            |_| None,
        )
        .unwrap_err();
        assert_eq!(
            locals_error,
            StackMapExpansionError::LocalsWidth {
                offset: Some(0),
                width: 2,
                limit: 1,
            }
        );

        let stack_error = expand_stack_map_table(
            &StackMapTableAttribute {
                frames: vec![StackMapFrame::SameLocalsOneStack {
                    frame_type: 64,
                    stack: ClassfileVerificationType::Double,
                }],
            },
            &input,
            &code,
            |_| None,
        )
        .unwrap_err();
        assert_eq!(
            stack_error,
            StackMapExpansionError::StackWidth {
                offset: 0,
                width: 2,
                limit: 1,
            }
        );
    }
