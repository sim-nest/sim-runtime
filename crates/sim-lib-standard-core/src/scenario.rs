//! Bounded, explicitly authorized characterization scenario execution.

use std::{collections::BTreeSet, sync::Arc};

use sim_kernel::{Cx, Datum, Error, Result, Symbol};

/// A semantic observation lane selected by a characterization scenario.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScenarioObservationLane {
    /// The returned value or stable failure record.
    ValueOrFailure,
    /// Ordered runtime events.
    Events,
    /// Ordered operation or library receipts.
    Receipts,
    /// The browseable Card face.
    Browse,
}

/// One ordered, semantic scenario input and the authority required to apply it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioInput {
    /// Stable identity of this input within the scenario.
    pub id: Symbol,
    /// Authority exercised while applying this input.
    pub authority: Symbol,
    /// Canonical input data.
    pub datum: Datum,
}

impl ScenarioInput {
    /// Construct a declared input.
    pub fn new(id: Symbol, authority: Symbol, datum: Datum) -> Self {
        Self {
            id,
            authority,
            datum,
        }
    }
}

/// Hard bounds for one scenario execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioLimits {
    /// Maximum number of ordered inputs.
    pub max_inputs: usize,
    /// Maximum number of observations across selected lanes.
    pub max_observations: usize,
}

impl ScenarioLimits {
    /// Construct explicit scenario bounds.
    pub const fn new(max_inputs: usize, max_observations: usize) -> Self {
        Self {
            max_inputs,
            max_observations,
        }
    }
}

/// Complete metadata required before a repeatable scenario may execute.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioSpec {
    /// Stable scenario identity.
    pub id: Symbol,
    /// Stable identity of the installed setup, independent of host state.
    pub setup: Symbol,
    /// Every authority the scenario is permitted to exercise.
    pub authorities: BTreeSet<Symbol>,
    /// Required execution bounds; `None` is rejected by preflight.
    pub limits: Option<ScenarioLimits>,
    /// Ordered semantic inputs.
    pub inputs: Vec<ScenarioInput>,
    /// Explicit observation lanes.
    pub observation_lanes: BTreeSet<ScenarioObservationLane>,
}

impl ScenarioSpec {
    /// Start a scenario declaration with stable scenario and setup identities.
    pub fn new(id: Symbol, setup: Symbol) -> Self {
        Self {
            id,
            setup,
            authorities: BTreeSet::new(),
            limits: None,
            inputs: Vec::new(),
            observation_lanes: BTreeSet::new(),
        }
    }

    /// Declare an authority available to the scenario.
    pub fn with_authority(mut self, authority: Symbol) -> Self {
        self.authorities.insert(authority);
        self
    }

    /// Declare hard execution bounds.
    pub fn with_limits(mut self, limits: ScenarioLimits) -> Self {
        self.limits = Some(limits);
        self
    }

    /// Append an ordered semantic input.
    pub fn with_input(mut self, input: ScenarioInput) -> Self {
        self.inputs.push(input);
        self
    }

    /// Select an observation lane.
    pub fn observing(mut self, lane: ScenarioObservationLane) -> Self {
        self.observation_lanes.insert(lane);
        self
    }

    pub(crate) fn validate(
        &self,
        supported_lanes: &BTreeSet<ScenarioObservationLane>,
    ) -> Result<()> {
        let Some(limits) = self.limits else {
            return Err(Error::Eval(format!(
                "scenario {} is missing limits",
                self.id
            )));
        };
        if limits.max_inputs == 0 || limits.max_observations == 0 {
            return Err(Error::Eval(format!(
                "scenario {} has a zero bound",
                self.id
            )));
        }
        if self.inputs.len() > limits.max_inputs {
            return Err(Error::Eval(format!(
                "scenario {} exceeds its input bound",
                self.id
            )));
        }
        if self.observation_lanes.len() > limits.max_observations {
            return Err(Error::Eval(format!(
                "scenario {} exceeds its observation bound",
                self.id
            )));
        }
        if self.observation_lanes.is_empty() {
            return Err(Error::Eval(format!(
                "scenario {} selects no observation lanes",
                self.id
            )));
        }
        if let Some(input) = self
            .inputs
            .iter()
            .find(|input| !self.authorities.contains(&input.authority))
        {
            return Err(Error::Eval(format!(
                "scenario {} input {} uses undeclared authority {}",
                self.id, input.id, input.authority
            )));
        }
        if let Some(lane) = self
            .observation_lanes
            .iter()
            .find(|lane| !supported_lanes.contains(lane))
        {
            return Err(Error::Eval(format!(
                "scenario {} selects unsupported lane {lane:?}",
                self.id
            )));
        }
        Ok(())
    }
}

/// Effectful body of a scenario, invoked only after batch preflight succeeds.
pub type ScenarioDriver = Arc<dyn Fn(&mut Cx, &ScenarioSpec) -> Result<()> + Send + Sync + 'static>;

/// Registered scenario metadata and its bounded driver.
#[derive(Clone)]
pub struct CharacterizationScenario {
    /// Explicit scenario contract.
    pub spec: ScenarioSpec,
    pub(crate) driver: ScenarioDriver,
}

impl CharacterizationScenario {
    /// Pair a scenario contract with its driver.
    pub fn new(spec: ScenarioSpec, driver: ScenarioDriver) -> Self {
        Self { spec, driver }
    }
}
