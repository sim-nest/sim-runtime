//! Immutable, content-addressed characterization captures.

use sim_kernel::{
    Claim, ClaimKind, ClaimPattern, Cx, Datum, DatumStore, Error, Ref, Result, Symbol,
    card::card_kind_predicate, standard::standard_evidence_predicate,
};

use crate::{
    BoundedLane, CanonicalFailure, CanonicalObservation, CanonicalOutcome, FailureLocation,
    ScenarioObservationLane, ScenarioSpec,
};

/// Schema tag for the first characterization capture datum.
pub fn characterization_capture_kind() -> Symbol {
    Symbol::qualified("standard", "characterization-capture/v1")
}

/// Claim predicate relating a scenario to one immutable capture.
pub fn characterization_capture_predicate() -> Symbol {
    Symbol::qualified("standard", "characterization-capture")
}

/// A capture ready for validation and content-addressed publication.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CharacterizationCapture {
    /// Versioned schema tag. Unknown versions fail closed.
    pub schema: Symbol,
    /// Stable identity of the projection applied before capture.
    pub projection: Symbol,
    /// Complete canonical observations.
    pub observation: CanonicalObservation,
}

impl CharacterizationCapture {
    /// Construct a capture using the current schema.
    pub fn new(projection: Symbol, observation: CanonicalObservation) -> Self {
        Self {
            schema: characterization_capture_kind(),
            projection,
            observation,
        }
    }
}

/// Validate, intern, and publish an immutable capture for `scenario`.
///
/// Identity is derived solely from the versioned semantic datum. Rendered
/// cards, debug output, and host formatting never enter the datum.
pub fn publish_characterization_capture(
    cx: &mut Cx,
    scenario: &ScenarioSpec,
    capture: &CharacterizationCapture,
) -> Result<Ref> {
    validate_capture(scenario, capture)?;
    let capture_ref = cx
        .datum_store_mut()
        .intern(capture_datum(scenario, capture))
        .map(Ref::Content)?;

    insert_observed_once(
        cx,
        capture_ref.clone(),
        card_kind_predicate(),
        Ref::Symbol(characterization_capture_kind()),
    )?;
    insert_observed_once(
        cx,
        Ref::Symbol(scenario.id.clone()),
        characterization_capture_predicate(),
        capture_ref.clone(),
    )?;
    insert_observed_once(
        cx,
        Ref::Symbol(scenario.id.clone()),
        standard_evidence_predicate(),
        capture_ref.clone(),
    )?;
    Ok(capture_ref)
}

fn validate_capture(scenario: &ScenarioSpec, capture: &CharacterizationCapture) -> Result<()> {
    if capture.schema != characterization_capture_kind() {
        return Err(Error::Eval(format!(
            "unsupported characterization capture schema {}",
            capture.schema
        )));
    }
    let limits = scenario
        .limits
        .ok_or_else(|| Error::Eval(format!("scenario {} is missing limits", scenario.id)))?;

    validate_outcome_lane(scenario, &capture.observation)?;
    let mut observed = usize::from(capture.observation.outcome.is_some());
    observed += validate_lane(
        scenario,
        ScenarioObservationLane::Events,
        &capture.observation.events,
    )?;
    observed += validate_lane(
        scenario,
        ScenarioObservationLane::Receipts,
        &capture.observation.receipts,
    )?;
    observed += validate_lane(
        scenario,
        ScenarioObservationLane::Browse,
        &capture.observation.browse,
    )?;
    if observed > limits.max_observations {
        return Err(Error::Eval(format!(
            "scenario {} capture exceeds its observation bound",
            scenario.id
        )));
    }
    Ok(())
}

fn validate_outcome_lane(
    scenario: &ScenarioSpec,
    observation: &CanonicalObservation,
) -> Result<()> {
    let selected = scenario
        .observation_lanes
        .contains(&ScenarioObservationLane::ValueOrFailure);
    if selected != observation.outcome.is_some() {
        return Err(Error::Eval(format!(
            "scenario {} capture has incomplete value-or-failure observations",
            scenario.id
        )));
    }
    Ok(())
}

fn validate_lane<T>(
    scenario: &ScenarioSpec,
    lane: ScenarioObservationLane,
    observed: &BoundedLane<T>,
) -> Result<usize> {
    let selected = scenario.observation_lanes.contains(&lane);
    match (selected, observed) {
        (false, BoundedLane::Absent) => Ok(0),
        (true, BoundedLane::Complete(items)) => Ok(items.len()),
        (true, BoundedLane::Truncated { .. }) => Err(Error::Eval(format!(
            "scenario {} capture contains truncated {lane:?} observations",
            scenario.id
        ))),
        _ => Err(Error::Eval(format!(
            "scenario {} capture has incomplete {lane:?} observations",
            scenario.id
        ))),
    }
}

fn capture_datum(scenario: &ScenarioSpec, capture: &CharacterizationCapture) -> Datum {
    Datum::Node {
        tag: capture.schema.clone(),
        fields: vec![
            ("scenario", Datum::Symbol(scenario.id.clone())),
            ("setup", Datum::Symbol(scenario.setup.clone())),
            (
                "inputs",
                Datum::List(
                    scenario
                        .inputs
                        .iter()
                        .map(|input| Datum::Node {
                            tag: Symbol::qualified("standard", "characterization-input/v1"),
                            fields: vec![
                                ("id", Datum::Symbol(input.id.clone())),
                                ("authority", Datum::Symbol(input.authority.clone())),
                                ("datum", input.datum.clone()),
                            ]
                            .into_iter()
                            .map(symbol_field)
                            .collect(),
                        })
                        .collect(),
                ),
            ),
            ("projection", Datum::Symbol(capture.projection.clone())),
            ("observation", observation_datum(&capture.observation)),
        ]
        .into_iter()
        .map(symbol_field)
        .collect(),
    }
}

fn observation_datum(observation: &CanonicalObservation) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("standard", "characterization-observation/v1"),
        fields: vec![
            (
                "outcome",
                optional_outcome_datum(observation.outcome.as_ref()),
            ),
            ("events", lane_datum(&observation.events)),
            ("receipts", lane_datum(&observation.receipts)),
            ("browse", lane_datum(&observation.browse)),
        ]
        .into_iter()
        .map(symbol_field)
        .collect(),
    }
}

fn optional_outcome_datum(outcome: Option<&CanonicalOutcome>) -> Datum {
    match outcome {
        None => Datum::Node {
            tag: Symbol::qualified("standard/capture", "absent"),
            fields: Vec::new(),
        },
        Some(CanonicalOutcome::Success(value)) => Datum::Node {
            tag: Symbol::qualified("standard/capture", "success"),
            fields: vec![(Symbol::new("value"), value.clone())],
        },
        Some(CanonicalOutcome::Failure(failure)) => failure_datum(failure),
    }
}

fn failure_datum(failure: &CanonicalFailure) -> Datum {
    Datum::Node {
        tag: Symbol::qualified("standard/capture", "failure"),
        fields: vec![
            ("class", Datum::Symbol(failure.class.clone())),
            ("detail", failure.detail.clone()),
            ("location", location_datum(failure.location.as_ref())),
        ]
        .into_iter()
        .map(symbol_field)
        .collect(),
    }
}

fn location_datum(location: Option<&FailureLocation>) -> Datum {
    match location {
        None => Datum::Nil,
        Some(location) => Datum::Node {
            tag: Symbol::qualified("standard/capture", "location"),
            fields: vec![
                (
                    Symbol::new("source"),
                    Datum::Symbol(location.source.clone()),
                ),
                (
                    Symbol::new("start"),
                    Datum::String(location.start.to_string()),
                ),
                (Symbol::new("end"), Datum::String(location.end.to_string())),
            ],
        },
    }
}

fn lane_datum(lane: &BoundedLane<Datum>) -> Datum {
    match lane {
        BoundedLane::Absent => Datum::Node {
            tag: Symbol::qualified("standard/capture", "absent"),
            fields: Vec::new(),
        },
        BoundedLane::Complete(items) => Datum::Node {
            tag: Symbol::qualified("standard/capture", "complete"),
            fields: vec![(Symbol::new("items"), Datum::List(items.clone()))],
        },
        BoundedLane::Truncated { items, omitted } => Datum::Node {
            tag: Symbol::qualified("standard/capture", "truncated"),
            fields: vec![
                (Symbol::new("items"), Datum::List(items.clone())),
                (Symbol::new("omitted"), Datum::String(omitted.to_string())),
            ],
        },
    }
}

fn symbol_field((name, datum): (&str, Datum)) -> (Symbol, Datum) {
    (Symbol::new(name), datum)
}

fn insert_observed_once(cx: &mut Cx, subject: Ref, predicate: Symbol, object: Ref) -> Result<()> {
    if cx
        .query_facts(ClaimPattern::exact(
            subject.clone(),
            predicate.clone(),
            object.clone(),
        ))?
        .is_empty()
    {
        cx.insert_fact(Claim::public(subject, predicate, object).with_kind(ClaimKind::Observed))?;
    }
    Ok(())
}
