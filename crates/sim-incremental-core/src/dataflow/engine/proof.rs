fn previous_seed_fingerprints<N, E, L, C, S>(
    previous: &DataflowCompletionProof<N, E, C, S>,
    _graph: &DataflowGraph<N, E, L, C>,
    _bottom: &S,
) -> BTreeMap<N, ValueFingerprint>
where
    N: Clone + Hash + Ord,
    E: Clone + Hash + Ord,
    L: Hash + Ord,
    C: Hash + Ord,
    S: Hash,
{
    previous.seed_fingerprints.clone()
}
#[derive(Clone, Copy)]
struct ProofInputs {
    graph: ValueFingerprint,
    lattice: ValueFingerprint,
    policy: ValueFingerprint,
    boundaries: ValueFingerprint,
    limits: ValueFingerprint,
    dependencies: ValueFingerprint,
}

fn proof_inputs<N, E, L, C, S, P>(
    graph: &DataflowGraph<N, E, L, C>,
    transfer: &AdmittedTransfer<P>,
    bottom: &S,
    seeds: &BTreeMap<N, S>,
    budgets: QueryBudgets,
) -> ProofInputs
where
    N: Clone + Hash + Ord,
    E: Clone + Hash + Ord,
    L: Hash + Ord,
    C: Hash + Ord,
    S: Hash,
{
    ProofInputs {
        graph: graph.fingerprint(),
        lattice: bottom.incremental_fingerprint(),
        policy: transfer.fingerprint(),
        boundaries: graph
            .nodes()
            .map(|node| (node.id(), node.boundary()))
            .collect::<Vec<_>>()
            .incremental_fingerprint(),
        limits: (
            budgets.max_work,
            budgets.max_observations,
            budgets.max_depth,
            budgets.max_output,
        )
            .incremental_fingerprint(),
        dependencies: seeds.incremental_fingerprint(),
    }
}

fn mint_completion_proof<N, E, L, C, S, P>(
    graph: &DataflowGraph<N, E, L, C>,
    transfer: &AdmittedTransfer<P>,
    bottom: &S,
    seeds: &BTreeMap<N, S>,
    budgets: QueryBudgets,
    solution: DataflowSolution<N, E, C, S>,
) -> DataflowCompletionProof<N, E, C, S>
where
    N: Clone + Hash + Ord,
    E: Clone + Hash + Ord,
    L: Hash + Ord,
    C: Hash + Ord,
    S: Hash,
{
    let inputs = proof_inputs(graph, transfer, bottom, seeds, budgets);
    let node_fingerprints = solution
        .states
        .iter()
        .map(|(node, state)| (node.clone(), state.incremental_fingerprint()))
        .collect::<BTreeMap<_, _>>();
    let seed_fingerprints = graph
        .nodes()
        .map(|node| {
            (
                node.id().clone(),
                seeds
                    .get(node.id())
                    .unwrap_or(bottom)
                    .incremental_fingerprint(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let observations = solution
        .events
        .iter()
        .filter_map(|event| match event {
            DataflowEvent::Propagate { edge, .. } => {
                let edge_record = graph.edge(edge).expect("solution edge belongs to graph");
                let (source, target) = edge_record.predecessor_and_successor();
                Some((source.clone(), edge.clone(), target.clone()))
            }
            DataflowEvent::Visit(_) => None,
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let identity = (
        DATAFLOW_PROOF_SCHEMA_REVISION,
        inputs.graph,
        inputs.lattice,
        inputs.policy,
        inputs.boundaries,
        inputs.limits,
        inputs.dependencies,
        &observations,
        &node_fingerprints,
    )
        .incremental_fingerprint();
    DataflowCompletionProof {
        identity,
        graph: inputs.graph,
        lattice: inputs.lattice,
        policy: inputs.policy,
        boundaries: inputs.boundaries,
        limits: inputs.limits,
        dependencies: inputs.dependencies,
        seed_fingerprints,
        observations,
        node_fingerprints,
        solution,
    }
}

fn check_fingerprint<N, E, L>(
    changed: ContinuationFingerprint,
    expected: ValueFingerprint,
    actual: ValueFingerprint,
) -> Result<(), DataflowError<N, E, L>> {
    if expected == actual {
        Ok(())
    } else {
        Err(DataflowError::ContinuationMismatch {
            changed,
            expected,
            actual,
        })
    }
}

fn make_continuation<N, E, C, S>(
    identity: ContinuationIdentity,
    states: BTreeMap<N, S>,
    pending: BTreeSet<N>,
    events: Vec<DataflowEvent<N, E, C>>,
    causes: BTreeMap<N, CausalRecord<N, E>>,
    usage: DataflowUsage,
    cause_limit: usize,
) -> DataflowContinuation<N, E, C, S>
where
    N: Hash + Ord,
    E: Hash,
    C: Hash,
    S: Hash,
{
    let ContinuationIdentity {
        graph,
        policy,
        dependencies,
    } = identity;
    let token = ContinuationToken::new(
        (
            graph,
            policy,
            dependencies,
            &states,
            &pending,
            &events,
            &causes,
            cause_limit,
        )
            .incremental_fingerprint()
            .get(),
    );
    DataflowContinuation {
        token,
        graph,
        policy,
        dependencies,
        states,
        pending,
        events,
        causes,
        cause_limit,
        usage,
    }
}

fn record_cause<N: Ord, E>(
    causes: &mut BTreeMap<N, CausalRecord<N, E>>,
    node: N,
    cause: CausalPredecessor<N, E>,
    limit: usize,
) {
    let record = causes.entry(node).or_insert_with(|| CausalRecord {
        retained: Vec::new(),
        omitted: 0,
    });
    if record.retained.len() < limit {
        record.retained.push(cause);
    } else {
        record.omitted = record.omitted.saturating_add(1);
    }
}

fn replace_state<N: Ord, S: super::StateSize, E, L: Clone>(
    states: &mut BTreeMap<N, S>,
    node: N,
    state: S,
    usage: &mut DataflowUsage,
    budgets: QueryBudgets,
    location: L,
) -> Result<(), DataflowError<N, E, L>> {
    charge(
        &mut usage.output,
        state.state_size(),
        budgets.max_output,
        BudgetKind::Output,
        ChargeLocation {
            node: None,
            edge: None,
            location,
            target_location: None,
        },
    )?;
    states.insert(node, state);
    Ok(())
}

fn charge_work<N, E, L: Clone>(
    usage: &mut DataflowUsage,
    budgets: QueryBudgets,
    node: Option<N>,
    location: L,
) -> Result<(), DataflowError<N, E, L>> {
    charge(
        &mut usage.work,
        1,
        budgets.max_work,
        BudgetKind::Work,
        ChargeLocation {
            node,
            edge: None,
            location,
            target_location: None,
        },
    )
}

fn charge_work_edge<N, E, L: Clone>(
    usage: &mut DataflowUsage,
    budgets: QueryBudgets,
    edge: E,
    location: L,
    target: L,
) -> Result<(), DataflowError<N, E, L>> {
    charge(
        &mut usage.work,
        1,
        budgets.max_work,
        BudgetKind::Work,
        ChargeLocation {
            node: None,
            edge: Some(edge),
            location,
            target_location: Some(target),
        },
    )
}

fn charge<N, E, L>(
    counter: &mut usize,
    units: usize,
    limit: usize,
    kind: BudgetKind,
    at: ChargeLocation<N, E, L>,
) -> Result<(), DataflowError<N, E, L>> {
    let attempted = counter.saturating_add(units);
    if attempted > limit {
        return Err(DataflowError::BudgetExceeded {
            kind,
            limit,
            attempted,
            node: at.node,
            edge: at.edge,
            location: at.location,
            target_location: at.target_location,
        });
    }
    *counter = attempted;
    Ok(())
}
