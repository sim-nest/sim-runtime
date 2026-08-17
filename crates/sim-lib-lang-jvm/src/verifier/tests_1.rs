    use super::*;

    #[test]
    fn generated_verifier_rules_cover_every_shared_opcode_once() {
        assert_eq!(VERIFIER_RULES.len(), sim_codec_classfile::OPCODES.len());
        for (byte, (rule, metadata)) in VERIFIER_RULES
            .iter()
            .zip(sim_codec_classfile::OPCODES.iter())
            .enumerate()
        {
            assert_eq!(rule.opcode, metadata.opcode, "opcode byte {byte:#04x}");
            assert_eq!(verifier_rule(metadata.opcode), rule);
        }
        for opcode in [
            Opcode::Jsr,
            Opcode::Ret,
            Opcode::JsrW,
            Opcode::Breakpoint,
            Opcode::ReservedCB,
            Opcode::Impdep1,
            Opcode::Impdep2,
        ] {
            assert_eq!(
                verifier_rule(opcode).family,
                VerifierRuleFamily::ExplicitRefusal,
                "{opcode:?} must be refused explicitly"
            );
        }
    }
    use crate::{
        JvmInstructionPolicy, JvmInstructionSemantics, JvmSlotKind, PreparationError, prepare_code,
    };
    use sim_codec_classfile::{
        ByteReader, CodeException, ConstantPool, InstructionErrorKind, Opcode, decode_instructions,
    };
    use sim_incremental_core::dataflow::{EdgeClass, LawSuite};
    use sim_kernel::{Cx, DefaultFactory, NoopEvalPolicy, SourceId};

    const NONE: &[JvmSlotKind] = &[];
    const INT: &[JvmSlotKind] = &[JvmSlotKind::CategoryOne];

    struct GraphPolicy;

    impl JvmInstructionPolicy for GraphPolicy {
        fn semantics(opcode: Opcode) -> Option<JvmInstructionSemantics> {
            let (pops, pushes) = match opcode {
                Opcode::Iconst0 => (NONE, INT),
                Opcode::Idiv => (
                    &[JvmSlotKind::CategoryOne, JvmSlotKind::CategoryOne][..],
                    INT,
                ),
                Opcode::Ireturn => (INT, NONE),
                Opcode::Ifeq => (INT, NONE),
                Opcode::New => (NONE, INT),
                Opcode::Invokespecial => (INT, NONE),
                Opcode::Return | Opcode::Goto | Opcode::Jsr | Opcode::Ret => (NONE, NONE),
                _ if verifier_rule(opcode).family == VerifierRuleFamily::ConstantsLocalsStack => {
                    (NONE, NONE)
                }
                _ if verifier_rule(opcode).family == VerifierRuleFamily::NumericConversion => {
                    (NONE, NONE)
                }
                _ if verifier_rule(opcode).family == VerifierRuleFamily::ObjectArrayField => {
                    (NONE, NONE)
                }
                _ => return None,
            };
            Some(JvmInstructionSemantics {
                pops,
                pushes,
                safepoint: false,
            })
        }
}
    fn empty_pool() -> ConstantPool {
        ConstantPool::decode(&mut ByteReader::new(&[0, 1], 1), 61).unwrap()
    }

    fn field_pool() -> ConstantPool {
        let bytes = [
            0, 7, 1, 0, 5, b'O', b'w', b'n', b'e', b'r', 7, 0, 1, 1, 0, 5, b'v', b'a', b'l', b'u',
            b'e', 1, 0, 1, b'I', 12, 0, 3, 0, 4, 9, 0, 2, 0, 5,
        ];
        ConstantPool::decode(&mut ByteReader::new(&bytes, bytes.len()), 61).unwrap()
    }

    fn invocation_pool(tag: u8) -> ConstantPool {
        let mut bytes = vec![0, if tag == 18 { 6 } else { 7 }, tag];
        if tag == 18 {
            bytes.extend_from_slice(&[0, 0, 0, 2]);
            bytes.extend_from_slice(&[12, 0, 3, 0, 4]);
            bytes.extend_from_slice(&[
                1, 0, 4, b'w', b'o', b'r', b'k', 1, 0, 4, b'(', b'I', b')', b'J', 1, 0, 1, b'X',
            ]);
        } else {
            bytes.extend_from_slice(&[0, 2, 0, 3, 7, 0, 6, 12, 0, 4, 0, 5]);
            bytes.extend_from_slice(&[
                1, 0, 4, b'w', b'o', b'r', b'k', 1, 0, 4, b'(', b'I', b')', b'J', 1, 0, 12, b's',
                b'a', b'm', b'p', b'l', b'e', b'/', b'O', b'w', b'n', b'e', b'r',
            ]);
        }
        ConstantPool::decode(&mut ByteReader::new(&bytes, bytes.len()), 61).unwrap()
    }

    fn test_method(descriptor: &str, access_flags: u16) -> JavaMember {
        let cx = Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory));
        JavaClassMetadata::test_class(&cx, "Owner", &[], 0, &[("work", descriptor, access_flags)])
            .select_method("work", descriptor)
            .unwrap()
            .clone()
    }

    fn prepared(bytes: &[u8], handlers: &[CodeException]) -> LocatedCode<PreparedJvmPolicy> {
        prepared_with_pool(bytes, handlers, &empty_pool())
    }

    fn prepared_with_pool(
        bytes: &[u8],
        handlers: &[CodeException],
        pool: &ConstantPool,
    ) -> LocatedCode<PreparedJvmPolicy> {
        let decoded = decode_instructions(bytes, 61, pool).unwrap();
        prepare_code::<GraphPolicy>(
            &decoded,
            bytes.len(),
            handlers,
            SourceId("Verifier.graph()V".into()),
        )
        .unwrap()
    }

    #[test]
    fn graph_reuses_locations_and_only_throwing_instructions_reach_handlers() {
        let bytes = [
            Opcode::Iconst0 as u8,
            Opcode::Iconst0 as u8,
            Opcode::Idiv as u8,
            Opcode::Ireturn as u8,
        ];
        let handlers = [CodeException {
            start_pc: 0,
            end_pc: 3,
            handler_pc: 3,
            catch_type: 7,
        }];
        let code = prepared(&bytes, &handlers);
        let graph = build_verification_graph(&code).unwrap();

        assert_eq!(graph.nodes().len(), code.len());
        assert_eq!(
            graph.node(&0).unwrap().location().throw_capability,
            ThrowCapability::Never
        );
        assert_eq!(
            graph.node(&2).unwrap().location().throw_capability,
            ThrowCapability::MayThrow
        );
        let exceptional_sources = graph
            .edges()
            .filter_map(|edge| match edge.class() {
                EdgeClass::Custom(VerificationEdgeClass::Exceptional {
                    row: 0,
                    catch_type: 7,
                }) => Some(*edge.source()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(exceptional_sources, [2]);
    }

    #[test]
    fn allocation_sites_of_the_same_class_remain_distinct_types() {
        let pool = ConstantPool::decode(
            &mut ByteReader::new(
                &[
                    0, 3, 7, 0, 2, 1, 0, 12, b's', b'a', b'm', b'p', b'l', b'e', b'/', b'V', b'a',
                    b'l', b'u', b'e',
                ],
                64,
            ),
            61,
        )
        .unwrap();
        let code = prepared_with_pool(
            &[Opcode::New as u8, 0, 1, Opcode::New as u8, 0, 1],
            &[],
            &pool,
        );
        let initial = VerificationState {
            locals: VerificationFrame::new(FrameKind::Locals, 0),
            stack: VerificationFrame::new(FrameKind::OperandStack, 2),
        };
        let first = transfer_new_instruction(
            code.instruction(code.cursor(InstructionId(0)).unwrap())
                .instruction(),
            0,
            &initial,
        )
        .unwrap();
        let second = transfer_new_instruction(
            code.instruction(code.cursor(InstructionId(1)).unwrap())
                .instruction(),
            3,
            &first,
        )
        .unwrap();
        assert_eq!(
            stack_values(&second.stack),
            vec![
                VerificationType::Uninitialized(0),
                VerificationType::Uninitialized(1)
            ]
        );
    }

    #[test]
    fn successful_constructor_replaces_every_alias() {
        let pool_bytes = [
            0, 7, 10, 0, 2, 0, 3, 7, 0, 4, 12, 0, 5, 0, 6, 1, 0, 12, b's', b'a', b'm', b'p', b'l',
            b'e', b'/', b'V', b'a', b'l', b'u', b'e', 1, 0, 6, b'<', b'i', b'n', b'i', b't', b'>',
            1, 0, 3, b'(', b')', b'V',
        ];
        let pool =
            ConstantPool::decode(&mut ByteReader::new(&pool_bytes, pool_bytes.len()), 61).unwrap();
        let code = prepared_with_pool(&[Opcode::Invokespecial as u8, 0, 1], &[], &pool);
        let alias = VerificationType::Uninitialized(7);
        let mut locals = VerificationFrame::new(FrameKind::Locals, 2);
        locals.set_local(0, alias.clone()).unwrap();
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 2);
        stack.push(alias.clone()).unwrap();
        stack.push(alias).unwrap();
        let next = transfer_constructor_instruction(
            code.instruction(code.cursor(InstructionId(0)).unwrap())
                .instruction(),
            0,
            &VerificationState { locals, stack },
            &VerificationConstructor {
                owner: "sample/Value".into(),
                name: "<init>".into(),
                descriptor: "()V".into(),
                receiver: VerificationType::Uninitialized(7),
            },
        )
        .unwrap();
        let initialized = VerificationType::Reference(ReferenceType::Class("sample/Value".into()));
        assert_eq!(next.locals.get(0), Some(&initialized));
        assert_eq!(stack_values(&next.stack), vec![initialized]);
    }

    #[test]
    fn every_invocation_kind_checks_descriptor_receiver_and_owner_kind() {
        let pool = invocation_pool(10);
        let loader = crate::ClassLoader::new(1);
        let environment = VerificationEnvironment::new(&loader, 1);
        let cases = [
            (Opcode::Invokevirtual, false, 0),
            (Opcode::Invokespecial, false, 0),
            (Opcode::Invokestatic, false, 0x0008),
        ];
        for (opcode, owner_is_interface, flags) in cases {
            let suffix: &[u8] = if opcode == Opcode::Invokeinterface {
                &[2, 0]
            } else {
                &[]
            };
            let bytes = [opcode as u8, 0, 1]
                .into_iter()
                .chain(suffix.iter().copied())
                .collect::<Vec<_>>();
            let code = prepared_with_pool(&bytes, &[], &pool);
            let method = test_method("(I)J", flags);
            let mut stack = VerificationFrame::new(FrameKind::OperandStack, 4);
            if opcode != Opcode::Invokestatic {
                stack
                    .push(VerificationType::Reference(ReferenceType::Class(
                        "Owner".into(),
                    )))
                    .unwrap();
            }
            stack.push(VerificationType::Int).unwrap();
            let next = transfer_invocation_instruction(
                code.instruction(code.cursor(InstructionId(0)).unwrap())
                    .instruction(),
                0,
                &VerificationState {
                    locals: VerificationFrame::new(FrameKind::Locals, 0),
                    stack,
                },
                &VerificationInvocation {
                    owner: "Owner",
                    owner_is_interface,
                    method: &method,
                    accessible: true,
                    signature_polymorphic: false,
                },
                &environment,
                1,
            )
            .unwrap();
            assert_eq!(stack_values(&next.stack), [VerificationType::Long]);
        }

        let pool = invocation_pool(11);
        let code = prepared_with_pool(&[Opcode::Invokeinterface as u8, 0, 1, 2, 0], &[], &pool);
        let method = test_method("(I)J", 0);
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 3);
        stack.push(VerificationType::Null).unwrap();
        stack.push(VerificationType::Int).unwrap();
        transfer_invocation_instruction(
            code.instruction(code.cursor(InstructionId(0)).unwrap())
                .instruction(),
            0,
            &VerificationState {
                locals: VerificationFrame::new(FrameKind::Locals, 0),
                stack,
            },
            &VerificationInvocation {
                owner: "Owner",
                owner_is_interface: true,
                method: &method,
                accessible: true,
                signature_polymorphic: false,
            },
            &environment,
            1,
        )
        .unwrap();
    }

    #[test]
    fn dynamic_verification_reuses_executor_identity_without_linkage() {
        let pool = invocation_pool(18);
        let code = prepared_with_pool(&[Opcode::Invokedynamic as u8, 0, 1, 0, 0], &[], &pool);
        let instruction = code
            .instruction(code.cursor(InstructionId(0)).unwrap())
            .instruction();
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 3);
        stack.push(VerificationType::Int).unwrap();
        let state = VerificationState {
            locals: VerificationFrame::new(FrameKind::Locals, 0),
            stack,
        };
        let refused = DynamicBootstrap {
            owner: "sample/Bootstrap".into(),
            name: "link".into(),
            descriptor: "()V".into(),
        };
        let error = transfer_dynamic_invocation_instruction(
            instruction,
            0,
            &state,
            &VerificationDynamicInvocation {
                bootstrap: &refused,
                descriptor: "(I)J",
            },
        )
        .unwrap_err();
        assert_eq!(
            error.kind,
            VerificationTransferKind::DynamicBootstrap(DynamicLinkError::UnadmittedBootstrap {
                owner: refused.owner.clone(),
                name: refused.name.clone(),
                descriptor: refused.descriptor.clone(),
            })
        );

        let cache = crate::DynamicLinkCache::new();
        let admitted = DynamicBootstrap {
            owner: STRING_CONCAT_BOOTSTRAP_OWNER.into(),
            name: STRING_CONCAT_BOOTSTRAP_NAME.into(),
            descriptor: STRING_CONCAT_BOOTSTRAP_DESCRIPTOR.into(),
        };
        let next = transfer_dynamic_invocation_instruction(
            instruction,
            0,
            &state,
            &VerificationDynamicInvocation {
                bootstrap: &admitted,
                descriptor: "(I)J",
            },
        )
        .unwrap();
        assert_eq!(stack_values(&next.stack), [VerificationType::Long]);
        assert_eq!(
            cache.live_len(),
            0,
            "verification must not link or allocate a cache entry"
        );
    }

    #[test]
    fn initialized_uninitialized_backward_merge_is_refused() {
        let mut left = VerificationFrame::new(FrameKind::Locals, 1);
        left.set_local(0, VerificationType::Uninitialized(2))
            .unwrap();
        let mut right = VerificationFrame::new(FrameKind::Locals, 1);
        right
            .set_local(
                0,
                VerificationType::Reference(ReferenceType::Class("sample/Value".into())),
            )
            .unwrap();
        let error = join_initialization_states(
            InstructionId(1),
            4,
            &VerificationState {
                locals: left,
                stack: VerificationFrame::new(FrameKind::OperandStack, 0),
            },
            &VerificationState {
                locals: right,
                stack: VerificationFrame::new(FrameKind::OperandStack, 0),
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, VerificationTransferKind::InitializationMerge);
    }

    #[test]
    fn handler_entry_refuses_a_live_uninitialized_alias() {
        let mut locals = VerificationFrame::new(FrameKind::Locals, 1);
        locals
            .set_local(0, VerificationType::Uninitialized(4))
            .unwrap();
        let error = handler_entry_state(
            InstructionId(3),
            8,
            &VerificationState {
                locals,
                stack: VerificationFrame::new(FrameKind::OperandStack, 1),
            },
            ReferenceType::Class("java/lang/Throwable".into()),
        )
        .unwrap_err();
        assert_eq!(
            error.kind,
            VerificationTransferKind::UninitializedHandlerEntry
        );
    }

    #[test]
    fn handler_entry_has_exact_single_catch_operand_and_preserves_locals() {
        let mut locals = VerificationFrame::new(FrameKind::Locals, 1);
        locals.set_local(0, VerificationType::Int).unwrap();
        let mut stack = VerificationFrame::new(FrameKind::OperandStack, 3);
        stack.push(VerificationType::Long).unwrap();
        let entered = handler_entry_state(
            InstructionId(4),
            9,
            &VerificationState {
                locals: locals.clone(),
                stack,
            },
            ReferenceType::Class("sample/Caught".into()),
        )
        .unwrap();
        assert_eq!(entered.locals, locals);
        assert_eq!(
            stack_values(&entered.stack),
            [VerificationType::Reference(ReferenceType::Class(
                "sample/Caught".into()
            ))]
        );
    }

    #[test]
    fn mid_instruction_handler_is_rejected_with_its_byte_offset() {
        let bytes = [Opcode::Goto as u8, 0, 3, Opcode::Return as u8];
        let decoded = decode_instructions(&bytes, 61, &empty_pool()).unwrap();
        let error = match prepare_code::<GraphPolicy>(
            &decoded,
            bytes.len(),
            &[CodeException {
                start_pc: 1,
                end_pc: 3,
                handler_pc: 3,
                catch_type: 0,
            }],
            SourceId("Verifier.badHandler()V".into()),
        ) {
            Ok(_) => panic!("mid-instruction handler must be rejected"),
            Err(error) => error,
        };
        let PreparationError::Classfile(error) = error else {
            panic!("handler validation must retain its classfile refusal")
        };
        assert_eq!(error.kind, InstructionErrorKind::InvalidHandler);
        assert_eq!(error.offset, 1);
    }

    #[test]
    fn illegal_fallthrough_and_legacy_subroutines_are_located() {
        let fallthrough = prepared(&[Opcode::Iconst0 as u8], &[]);
        assert_eq!(
            build_verification_graph(&fallthrough).unwrap_err(),
            VerificationGraphError::IllegalFallthrough {
                instruction: InstructionId(0),
                offset: 0
            }
        );

        let decoded = decode_instructions(
            &[Opcode::Jsr as u8, 0, 3, Opcode::Return as u8],
            61,
            &empty_pool(),
        )
        .unwrap();
        let legacy_error = match prepare_code::<GraphPolicy>(
            &decoded,
            4,
            &[],
            SourceId("Verifier.legacy()V".into()),
        ) {
            Ok(_) => panic!("legacy subroutine must be refused during preparation"),
            Err(error) => error,
        };
        assert_eq!(
            legacy_error,
            PreparationError::UnsupportedOpcode {
                opcode: Opcode::Jsr,
                offset: 0
            }
        );
    }
