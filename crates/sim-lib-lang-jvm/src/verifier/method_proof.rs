/// Refusal while checking joined dataflow states against declared target frames.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StackMapConstraintError {
    /// A classfile version requiring target frames omitted one.
    Missing {
        /// Target instruction lacking its required declaration.
        instruction: InstructionId,
    },
    /// A declaration has a different shape or is not a supertype of the inferred state.
    NotAssignable {
        /// Target instruction carrying the incompatible declaration.
        instruction: InstructionId,
    },
    /// An inferred target state was unavailable after dataflow completed.
    MissingInference {
        /// Target instruction absent from the completed solution.
        instruction: InstructionId,
    },
}

/// Diagnostic policy for exception-table rows that no reachable throwing instruction enters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnreachableHandlerPolicy {
    /// Preserve unreachable rows as method-proof diagnostics.
    Report,
    /// Refuse a method containing an unreachable exception-table row.
    Refuse,
}

/// A sealed proof that every reachable instruction and exceptional path in one method was checked.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MethodVerificationProof {
    fixpoint: ValueFingerprint,
    dependencies: Vec<ClassDefinitionId>,
    dependency_observations: Vec<Observation<ClassDefinitionId>>,
    unreachable_handlers: Box<[usize]>,
}

impl MethodVerificationProof {
    /// Content identity of the stable shared-dataflow fixpoint.
    pub const fn fixpoint(&self) -> ValueFingerprint {
        self.fixpoint
    }

    /// Loaded class definitions captured while validating catch assignability.
    pub fn dependencies(&self) -> &[ClassDefinitionId] {
        &self.dependencies
    }

    /// Exact content and class-space observations made while sealing the method.
    pub fn dependency_observations(&self) -> &[Observation<ClassDefinitionId>] {
        &self.dependency_observations
    }

    /// Exception-table rows that no reachable throwing instruction can enter.
    pub fn unreachable_handlers(&self) -> &[usize] {
        &self.unreachable_handlers
    }
}

/// Reason a whole-method proof could not be sealed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MethodVerificationError {
    /// The completion proof does not describe the supplied graph, policy, limits, or seeds.
    IncompleteFixpoint(CompletionProofMismatch),
    /// A catch type could not be resolved from its constant-pool identity.
    UnresolvedCatchType {
        /// Exception-table row.
        row: usize,
        /// Constant-pool class index.
        catch_type: u16,
    },
    /// A declared catch class is not assignable to `java/lang/Throwable`.
    CatchTypeNotThrowable {
        /// Exception-table row.
        row: usize,
        /// Constant-pool class index.
        catch_type: u16,
    },
    /// A bounded hierarchy query needed for catch validation failed.
    CatchTypeQuery {
        /// Exception-table row.
        row: usize,
        /// Bounded class-space refusal.
        error: VerificationQueryError,
    },
    /// The precise single-operand handler frame is absent from the converged target state.
    ExceptionalFrame {
        /// Exception-table row.
        row: usize,
        /// Handler target instruction.
        instruction: InstructionId,
    },
    /// A declared branch or handler target constraint was not proved.
    TargetConstraint(StackMapConstraintError),
    /// Policy requires every exception-table row to be reachable.
    UnreachableHandler {
        /// Exception-table row.
        row: usize,
    },
}

/// Seals a whole-method proof from the shared engine's content-bound completion signal.
#[allow(clippy::too_many_arguments)]
pub fn seal_method_verification<P, F>(
    proof: &DataflowCompletionProof<
        u32,
        VerificationEdgeId,
        VerificationEdgeClass,
        VerificationState,
    >,
    graph: &VerificationGraph,
    transfer: &AdmittedTransfer<P>,
    bottom: &VerificationState,
    seeds: impl IntoIterator<Item = (u32, VerificationState)>,
    budgets: QueryBudgets,
    environment: &VerificationEnvironment<'_>,
    mut catch_class: F,
    lineage_limit: usize,
    classfile_version: u16,
    declarations: &[ExpandedStackMapFrame],
    max_locals: usize,
    max_stack: usize,
    unreachable_policy: UnreachableHandlerPolicy,
) -> Result<MethodVerificationProof, MethodVerificationError>
where
    P: TransferPolicy<VerificationState>,
    F: FnMut(u16) -> Option<ReferenceType>,
{
    let solution = FixpointEngine::present(proof, graph, transfer, bottom, seeds, budgets)
        .map_err(MethodVerificationError::IncompleteFixpoint)?;
    let inferred = solution
        .states()
        .map(|(id, state)| (InstructionId(*id), state.clone()))
        .collect();
    check_stack_map_constraints(
        classfile_version,
        graph,
        &inferred,
        declarations,
        max_locals,
        max_stack,
    )
    .map_err(MethodVerificationError::TargetConstraint)?;

    let mut reached = std::collections::BTreeSet::new();
    let mut declared = std::collections::BTreeSet::new();
    for edge in graph.edges() {
        let EdgeClass::Custom(VerificationEdgeClass::Exceptional { row, catch_type }) =
            edge.class()
        else {
            continue;
        };
        declared.insert(*row);
        let exception = if *catch_type == 0 {
            ReferenceType::Class("java/lang/Throwable".into())
        } else {
            let caught =
                catch_class(*catch_type).ok_or(MethodVerificationError::UnresolvedCatchType {
                    row: *row,
                    catch_type: *catch_type,
                })?;
            match environment.reference_assignability(
                &caught,
                &ReferenceType::Class("java/lang/Throwable".into()),
                lineage_limit,
            ) {
                Ok(query) if query.value == VerificationAssignability::Assignable => {}
                Ok(_) => {
                    return Err(MethodVerificationError::CatchTypeNotThrowable {
                        row: *row,
                        catch_type: *catch_type,
                    });
                }
                Err(failure) => {
                    return Err(MethodVerificationError::CatchTypeQuery {
                        row: *row,
                        error: failure.error,
                    });
                }
            }
            caught
        };
        let Some(source) = solution.state(edge.source()) else {
            continue;
        };
        if source.locals.normalized_slots().is_none() {
            continue;
        }
        reached.insert(*row);
        let expected = handler_entry_state(
            InstructionId(*edge.source()),
            graph
                .node(edge.source())
                .expect("edge source exists")
                .location()
                .offset,
            source,
            exception,
        )
        .map_err(|_| MethodVerificationError::ExceptionalFrame {
            row: *row,
            instruction: InstructionId(*edge.target()),
        })?;
        let actual =
            solution
                .state(edge.target())
                .ok_or(MethodVerificationError::ExceptionalFrame {
                    row: *row,
                    instruction: InstructionId(*edge.target()),
                })?;
        if !expected.less_equal(actual) {
            return Err(MethodVerificationError::ExceptionalFrame {
                row: *row,
                instruction: InstructionId(*edge.target()),
            });
        }
    }
    let unreachable = declared.difference(&reached).copied().collect::<Vec<_>>();
    if unreachable_policy == UnreachableHandlerPolicy::Refuse
        && let Some(row) = unreachable.first()
    {
        return Err(MethodVerificationError::UnreachableHandler { row: *row });
    }
    let dependency_observations = environment
        .dependencies()
        .iter()
        .map(|dependency| {
            Observation::read(
                dependency.class().clone(),
                Revision::new(dependency.revision().number()),
                dependency.class().incremental_fingerprint(),
            )
        })
        .collect();
    Ok(MethodVerificationProof {
        fixpoint: proof.identity(),
        dependencies: environment
            .dependencies()
            .iter()
            .map(|dependency| dependency.class().clone())
            .collect(),
        dependency_observations,
        unreachable_handlers: unreachable.into_boxed_slice(),
    })
}
