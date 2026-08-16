impl FixpointEngine {
    /// Solves to a stable fixpoint and mints its content-bound completion proof.
    pub fn solve_proven<N, E, L, C, S, P>(
        graph: &DataflowGraph<N, E, L, C>,
        transfer: &AdmittedTransfer<P>,
        bottom: S,
        seeds: impl IntoIterator<Item = (N, S)>,
        budgets: QueryBudgets,
    ) -> CompletionProofResult<N, E, L, C, S>
    where
        N: Clone + Hash + Ord,
        E: Clone + Hash + Ord,
        L: Clone + Hash + Ord,
        C: Clone + Hash + Ord,
        S: JoinSemilattice + Hash,
        P: TransferPolicy<S>,
    {
        let seeds = seeds.into_iter().collect::<BTreeMap<_, _>>();
        let solution = Self::solve(
            graph,
            transfer,
            bottom.clone(),
            seeds
                .iter()
                .map(|(node, state)| (node.clone(), state.clone())),
            budgets,
        )?;
        Ok(mint_completion_proof(
            graph, transfer, &bottom, &seeds, budgets, solution,
        ))
    }

    /// Presents a proof only when every semantic input still matches.
    pub fn present<'a, N, E, L, C, S, P>(
        proof: &'a DataflowCompletionProof<N, E, C, S>,
        graph: &DataflowGraph<N, E, L, C>,
        transfer: &AdmittedTransfer<P>,
        bottom: &S,
        seeds: impl IntoIterator<Item = (N, S)>,
        budgets: QueryBudgets,
    ) -> Result<&'a DataflowSolution<N, E, C, S>, CompletionProofMismatch>
    where
        N: Clone + Hash + Ord,
        E: Clone + Hash + Ord,
        L: Hash + Ord,
        C: Hash + Ord,
        S: Hash,
        P: TransferPolicy<S>,
    {
        let seeds = seeds.into_iter().collect::<BTreeMap<_, _>>();
        let identities = proof_inputs(graph, transfer, bottom, &seeds, budgets);
        if proof.graph != identities.graph || proof.boundaries != identities.boundaries {
            return Err(CompletionProofMismatch::Graph);
        }
        if proof.lattice != identities.lattice {
            return Err(CompletionProofMismatch::Lattice);
        }
        if proof.policy != identities.policy {
            return Err(CompletionProofMismatch::Policy);
        }
        if proof.limits != identities.limits {
            return Err(CompletionProofMismatch::Limits);
        }
        if proof.dependencies != identities.dependencies {
            return Err(CompletionProofMismatch::Dependencies);
        }
        Ok(&proof.solution)
    }

    /// Recomputes exactly the observed successor cone of changed entry facts.
    ///
    /// Structural, lattice, policy, or limit changes are intentionally refused:
    /// callers must perform a clean solve for those semantic edits.  Dependency
    /// edits reuse unaffected states and their observations, then remint a proof
    /// from the complete final state rather than blessing the old witness.
    pub fn solve_incremental<N, E, L, C, S, P>(
        previous: &DataflowCompletionProof<N, E, C, S>,
        graph: &DataflowGraph<N, E, L, C>,
        transfer: &AdmittedTransfer<P>,
        bottom: S,
        seeds: impl IntoIterator<Item = (N, S)>,
        budgets: QueryBudgets,
    ) -> CompletionProofResult<N, E, L, C, S>
    where
        N: Clone + Hash + Ord,
        E: Clone + Hash + Ord,
        L: Clone + Hash + Ord,
        C: Clone + Hash + Ord,
        S: JoinSemilattice + Hash,
        P: TransferPolicy<S>,
    {
        let seeds = seeds.into_iter().collect::<BTreeMap<_, _>>();
        for node in seeds.keys() {
            if graph.node(node).is_none() {
                return Err(DataflowError::UnknownSeed(node.clone()));
            }
        }
        let inputs = proof_inputs(graph, transfer, &bottom, &seeds, budgets);
        if previous.graph != inputs.graph
            || previous.lattice != inputs.lattice
            || previous.policy != inputs.policy
            || previous.boundaries != inputs.boundaries
            || previous.limits != inputs.limits
        {
            return Self::solve_proven(graph, transfer, bottom, seeds, budgets);
        }
        if previous.dependencies == inputs.dependencies {
            return Ok(previous.clone());
        }

        let old_seeds = previous_seed_fingerprints(previous, graph, &bottom);
        let changed = graph
            .nodes()
            .filter_map(|node| {
                let current = seeds
                    .get(node.id())
                    .unwrap_or(&bottom)
                    .incremental_fingerprint();
                (old_seeds.get(node.id()).copied() != Some(current)).then(|| node.id().clone())
            })
            .collect::<BTreeSet<_>>();
        let mut affected = changed.clone();
        let mut frontier = changed;
        while let Some(node) = frontier.pop_first() {
            for edge_id in graph.successors(&node).expect("graph node has an index") {
                let edge = graph.edge(edge_id).expect("graph edge has an index");
                let (_, target) = edge.predecessor_and_successor();
                if affected.insert(target.clone()) {
                    frontier.insert(target.clone());
                }
            }
        }

        let mut states = previous.solution.states.clone();
        for node in &affected {
            states.insert(node.clone(), bottom.clone());
        }
        for node in &affected {
            let mut state = seeds.get(node).cloned().unwrap_or_else(|| bottom.clone());
            for edge_id in graph.predecessors(node).expect("graph node has an index") {
                let edge = graph.edge(edge_id).expect("graph edge has an index");
                let (source, _) = edge.predecessor_and_successor();
                if !affected.contains(source) {
                    state = state.join(&transfer.transfer(&states[source]));
                }
            }
            states.insert(node.clone(), state);
        }

        let mut pending = affected.clone();
        let mut events = previous
            .solution
            .events
            .iter()
            .filter(|event| match event {
                DataflowEvent::Visit(node) => !affected.contains(node),
                DataflowEvent::Propagate { edge, .. } => {
                    let edge = graph.edge(edge).expect("proof edge belongs to graph");
                    let (source, _) = edge.predecessor_and_successor();
                    !affected.contains(source)
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut usage = DataflowUsage {
            ..DataflowUsage::default()
        };
        charge(
            &mut usage.depth,
            states.len(),
            budgets.max_depth,
            BudgetKind::Depth,
            ChargeLocation {
                node: None,
                edge: None,
                location: graph
                    .nodes()
                    .next()
                    .expect("graphs are non-empty")
                    .location()
                    .clone(),
                target_location: None,
            },
        )?;
        while let Some(node) = pending.pop_first() {
            let location = graph
                .node(&node)
                .expect("affected node exists")
                .location()
                .clone();
            charge_work(&mut usage, budgets, Some(node.clone()), location.clone())?;
            events.push(DataflowEvent::Visit(node.clone()));
            charge_work(&mut usage, budgets, Some(node.clone()), location.clone())?;
            let output = transfer.transfer(&states[&node]);
            if !states[&node].less_equal(&output) {
                return Err(DataflowError::NodeFailure {
                    failure: DataflowFailure::Transfer,
                    node,
                    location,
                });
            }
            for edge_id in graph.successors(&node).expect("affected node has an index") {
                let edge = graph.edge(edge_id).expect("graph edge exists");
                let (_, target) = edge.predecessor_and_successor();
                let target_location = graph
                    .node(target)
                    .expect("edge target exists")
                    .location()
                    .clone();
                charge(
                    &mut usage.observations,
                    1,
                    budgets.max_observations,
                    BudgetKind::Observations,
                    ChargeLocation {
                        node: None,
                        edge: Some(edge_id.clone()),
                        location: location.clone(),
                        target_location: Some(target_location.clone()),
                    },
                )?;
                events.push(DataflowEvent::Propagate {
                    edge: edge_id.clone(),
                    class: edge.class().clone(),
                });
                charge_work_edge(
                    &mut usage,
                    budgets,
                    edge_id.clone(),
                    location.clone(),
                    target_location,
                )?;
                let joined = states[target].join(&output);
                if joined != states[target] {
                    states.insert(target.clone(), joined);
                    pending.insert(target.clone());
                }
            }
        }
        charge(
            &mut usage.output,
            states.values().map(super::StateSize::state_size).sum(),
            budgets.max_output,
            BudgetKind::Output,
            ChargeLocation {
                node: None,
                edge: None,
                location: graph
                    .nodes()
                    .next()
                    .expect("graphs are non-empty")
                    .location()
                    .clone(),
                target_location: None,
            },
        )?;
        let solution = DataflowSolution {
            states,
            events,
            usage,
            causes: BTreeMap::new(),
        };
        Ok(mint_completion_proof(
            graph, transfer, &bottom, &seeds, budgets, solution,
        ))
    }

    /// Solves from explicit entry/exit facts; nodes not reached from a seed stay at bottom.
    pub fn solve<N, E, L, C, S, P>(
        graph: &DataflowGraph<N, E, L, C>,
        transfer: &AdmittedTransfer<P>,
        bottom: S,
        seeds: impl IntoIterator<Item = (N, S)>,
        budgets: QueryBudgets,
    ) -> DataflowResult<N, E, L, C, S>
    where
        N: Clone + Hash + Ord,
        E: Clone + Hash + Ord,
        L: Clone + Hash + Ord,
        C: Clone + Hash + Ord,
        S: JoinSemilattice,
        P: TransferPolicy<S>,
    {
        let mut usage = DataflowUsage::default();
        let retained = graph.nodes().count();
        charge(
            &mut usage.depth,
            retained,
            budgets.max_depth,
            BudgetKind::Depth,
            ChargeLocation {
                node: None,
                edge: None,
                location: graph
                    .nodes()
                    .next()
                    .expect("graphs are non-empty")
                    .location()
                    .clone(),
                target_location: None,
            },
        )?;
        let initial_output = bottom.state_size().saturating_mul(retained);
        charge(
            &mut usage.output,
            initial_output,
            budgets.max_output,
            BudgetKind::Output,
            ChargeLocation {
                node: None,
                edge: None,
                location: graph
                    .nodes()
                    .next()
                    .expect("graphs are non-empty")
                    .location()
                    .clone(),
                target_location: None,
            },
        )?;
        let mut states = graph
            .nodes()
            .map(|node| (node.id().clone(), bottom.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut pending = BTreeSet::new();
        let mut causes = BTreeMap::<N, CausalRecord<N, E>>::new();
        for (node_id, seed) in seeds {
            let Some(node) = graph.node(&node_id) else {
                return Err(DataflowError::UnknownSeed(node_id));
            };
            let joined = states[&node_id].join(&seed);
            charge_work(
                &mut usage,
                budgets,
                Some(node_id.clone()),
                node.location().clone(),
            )?;
            if !states[&node_id].less_equal(&joined) || !seed.less_equal(&joined) {
                return Err(DataflowError::NodeFailure {
                    failure: DataflowFailure::Join,
                    node: node_id,
                    location: node.location().clone(),
                });
            }
            if joined != states[&node_id] {
                replace_state(
                    &mut states,
                    node_id.clone(),
                    joined,
                    &mut usage,
                    budgets,
                    node.location().clone(),
                )?;
                pending.insert(node_id.clone());
                record_cause(
                    &mut causes,
                    node_id.clone(),
                    CausalPredecessor {
                        node: node_id,
                        edge: None,
                    },
                    0,
                );
            }
        }

        let mut events = Vec::new();
        while let Some(node_id) = pending.pop_first() {
            let node = graph
                .node(&node_id)
                .expect("worklist node belongs to graph");
            charge_work(
                &mut usage,
                budgets,
                Some(node_id.clone()),
                node.location().clone(),
            )?;
            events.push(DataflowEvent::Visit(node_id.clone()));
            charge_work(
                &mut usage,
                budgets,
                Some(node_id.clone()),
                node.location().clone(),
            )?;
            let output = transfer.transfer(&states[&node_id]);
            if !states[&node_id].less_equal(&output) {
                return Err(DataflowError::NodeFailure {
                    failure: DataflowFailure::Transfer,
                    node: node_id,
                    location: node.location().clone(),
                });
            }
            for edge_id in graph.successors(&node_id).expect("node index is complete") {
                let edge = graph.edge(edge_id).expect("edge index is complete");
                let (_, target_id) = edge.predecessor_and_successor();
                let target = graph.node(target_id).expect("edge target exists");
                charge(
                    &mut usage.observations,
                    1,
                    budgets.max_observations,
                    BudgetKind::Observations,
                    ChargeLocation {
                        node: None,
                        edge: Some(edge_id.clone()),
                        location: node.location().clone(),
                        target_location: Some(target.location().clone()),
                    },
                )?;
                events.push(DataflowEvent::Propagate {
                    edge: edge_id.clone(),
                    class: edge.class().clone(),
                });
                charge_work_edge(
                    &mut usage,
                    budgets,
                    edge_id.clone(),
                    node.location().clone(),
                    target.location().clone(),
                )?;
                let joined = states[target_id].join(&output);
                if !states[target_id].less_equal(&joined) || !output.less_equal(&joined) {
                    return Err(DataflowError::EdgeFailure {
                        failure: DataflowFailure::Join,
                        edge: edge_id.clone(),
                        location: node.location().clone(),
                        target_location: target.location().clone(),
                    });
                }
                if joined != states[target_id] {
                    replace_state(
                        &mut states,
                        target_id.clone(),
                        joined,
                        &mut usage,
                        budgets,
                        target.location().clone(),
                    )?;
                    pending.insert(target_id.clone());
                    record_cause(
                        &mut causes,
                        target_id.clone(),
                        CausalPredecessor {
                            node: node_id.clone(),
                            edge: Some(edge_id.clone()),
                        },
                        0,
                    );
                }
            }
        }
        Ok(DataflowSolution {
            states,
            events,
            usage,
            causes,
        })
    }

    /// Starts a solve and executes at most `max_visits` complete worklist nodes.
    ///
    /// Unlike resource refusal inside [`Self::solve`], this cooperative boundary
    /// snapshots only between nodes, so resumption never repeats callbacks or
    /// presents a shortened event stream as complete.
    pub fn start_resumable<N, E, L, C, S, P>(
        graph: &DataflowGraph<N, E, L, C>,
        transfer: &AdmittedTransfer<P>,
        bottom: S,
        seeds: impl IntoIterator<Item = (N, S)>,
        max_visits: usize,
        explanation_limit: usize,
    ) -> DataflowProgressResult<N, E, L, C, S>
    where
        N: Clone + Hash + Ord,
        E: Clone + Hash + Ord,
        L: Clone + Hash + Ord,
        C: Clone + Hash + Ord,
        S: JoinSemilattice + Hash,
        P: TransferPolicy<S>,
    {
        let seeds = seeds.into_iter().collect::<BTreeMap<_, _>>();
        let dependencies = (&bottom, &seeds).incremental_fingerprint();
        let mut states = graph
            .nodes()
            .map(|node| (node.id().clone(), bottom.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut pending = BTreeSet::new();
        let mut causes = BTreeMap::<N, CausalRecord<N, E>>::new();
        for (node_id, seed) in seeds {
            if graph.node(&node_id).is_none() {
                return Err(DataflowError::UnknownSeed(node_id));
            }
            let joined = states[&node_id].join(&seed);
            if joined != states[&node_id] {
                states.insert(node_id.clone(), joined);
                pending.insert(node_id.clone());
                record_cause(
                    &mut causes,
                    node_id.clone(),
                    CausalPredecessor {
                        node: node_id,
                        edge: None,
                    },
                    explanation_limit,
                );
            }
        }
        let continuation = make_continuation(
            ContinuationIdentity {
                graph: graph.fingerprint(),
                policy: transfer.fingerprint(),
                dependencies,
            },
            states,
            pending,
            Vec::new(),
            causes,
            DataflowUsage::default(),
            explanation_limit,
        );
        Self::resume_bound(graph, transfer, continuation, max_visits)
    }

    /// Resumes a content-bound snapshot, refusing every changed input identity.
    pub fn resume<N, E, L, C, S, P>(
        graph: &DataflowGraph<N, E, L, C>,
        transfer: &AdmittedTransfer<P>,
        bottom: &S,
        seeds: impl IntoIterator<Item = (N, S)>,
        continuation: DataflowContinuation<N, E, C, S>,
        max_visits: usize,
    ) -> DataflowProgressResult<N, E, L, C, S>
    where
        N: Clone + Hash + Ord,
        E: Clone + Hash + Ord,
        L: Clone + Hash + Ord,
        C: Clone + Hash + Ord,
        S: JoinSemilattice + Hash,
        P: TransferPolicy<S>,
    {
        check_fingerprint(
            ContinuationFingerprint::Graph,
            continuation.graph,
            graph.fingerprint(),
        )?;
        check_fingerprint(
            ContinuationFingerprint::Policy,
            continuation.policy,
            transfer.fingerprint(),
        )?;
        let seeds = seeds.into_iter().collect::<BTreeMap<_, _>>();
        check_fingerprint(
            ContinuationFingerprint::Dependencies,
            continuation.dependencies,
            (bottom, &seeds).incremental_fingerprint(),
        )?;
        Self::resume_bound(graph, transfer, continuation, max_visits)
    }

    fn resume_bound<N, E, L, C, S, P>(
        graph: &DataflowGraph<N, E, L, C>,
        transfer: &AdmittedTransfer<P>,
        continuation: DataflowContinuation<N, E, C, S>,
        max_visits: usize,
    ) -> DataflowProgressResult<N, E, L, C, S>
    where
        N: Clone + Hash + Ord,
        E: Clone + Hash + Ord,
        L: Clone + Hash + Ord,
        C: Clone + Hash + Ord,
        S: JoinSemilattice + Hash,
        P: TransferPolicy<S>,
    {
        let DataflowContinuation {
            graph: graph_fingerprint,
            policy: policy_fingerprint,
            dependencies,
            mut states,
            mut pending,
            mut events,
            mut causes,
            mut usage,
            cause_limit,
            ..
        } = continuation;
        let mut visits = 0;
        while visits < max_visits {
            let Some(node_id) = pending.pop_first() else {
                return Ok(DataflowProgress::Complete(DataflowSolution {
                    states,
                    events,
                    usage,
                    causes,
                }));
            };
            visits += 1;
            usage.work = usage.work.saturating_add(2);
            events.push(DataflowEvent::Visit(node_id.clone()));
            let output = transfer.transfer(&states[&node_id]);
            let node = graph
                .node(&node_id)
                .expect("snapshot node belongs to graph");
            if !states[&node_id].less_equal(&output) {
                return Err(DataflowError::NodeFailure {
                    failure: DataflowFailure::Transfer,
                    node: node_id,
                    location: node.location().clone(),
                });
            }
            for edge_id in graph.successors(&node_id).expect("node index is complete") {
                let edge = graph.edge(edge_id).expect("edge index is complete");
                let (_, target_id) = edge.predecessor_and_successor();
                usage.observations = usage.observations.saturating_add(1);
                usage.work = usage.work.saturating_add(1);
                events.push(DataflowEvent::Propagate {
                    edge: edge_id.clone(),
                    class: edge.class().clone(),
                });
                let joined = states[target_id].join(&output);
                if joined != states[target_id] {
                    usage.output = usage.output.saturating_add(joined.state_size());
                    states.insert(target_id.clone(), joined);
                    pending.insert(target_id.clone());
                    record_cause(
                        &mut causes,
                        target_id.clone(),
                        CausalPredecessor {
                            node: node_id.clone(),
                            edge: Some(edge_id.clone()),
                        },
                        cause_limit,
                    );
                }
            }
        }
        if pending.is_empty() {
            Ok(DataflowProgress::Complete(DataflowSolution {
                states,
                events,
                usage,
                causes,
            }))
        } else {
            Ok(DataflowProgress::Suspended(make_continuation(
                ContinuationIdentity {
                    graph: graph_fingerprint,
                    policy: policy_fingerprint,
                    dependencies,
                },
                states,
                pending,
                events,
                causes,
                usage,
                cause_limit,
            )))
        }
    }
}
