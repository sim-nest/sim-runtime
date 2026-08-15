//! Immutable, content-addressed characterization captures.

use std::collections::BTreeSet;

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

/// A named, two-sided declaration of capture fields that are intentionally unstable.
///
/// Every ignored path must exist in both captures. The projection identity is
/// itself part of each capture and must match this declaration, so changing a
/// projection never silently preserves comparison equality.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureComparisonProjection {
    /// Stable identity recorded in both captures.
    pub identity: Symbol,
    /// Exact field paths omitted from both sides before comparison.
    pub unstable_fields: BTreeSet<String>,
}

impl CaptureComparisonProjection {
    /// Construct a named projection with no unstable fields.
    pub fn new(identity: Symbol) -> Self {
        Self {
            identity,
            unstable_fields: BTreeSet::new(),
        }
    }

    /// Declare one exact, two-sided unstable field path.
    pub fn ignoring(mut self, path: impl Into<String>) -> Self {
        self.unstable_fields.insert(path.into());
        self
    }
}

/// One exact behavioral difference between two characterization captures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureDifference {
    /// Stable path within the canonical comparison datum.
    pub path: String,
    /// Canonical value from the left capture.
    pub left: Datum,
    /// Canonical value from the right capture.
    pub right: Datum,
}

/// Strict, located result of comparing two characterization captures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureComparison {
    /// Projection applied symmetrically to both captures.
    pub projection: Symbol,
    /// All differences in stable path order.
    pub differences: Vec<CaptureDifference>,
}

impl CaptureComparison {
    /// Whether the projected captures are identical.
    pub fn is_same(&self) -> bool {
        self.differences.is_empty()
    }
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

/// Compare two captures recursively after applying one declared two-sided projection.
///
/// Schema, setup, ordered inputs, selected observation lanes, projection
/// identity, and observations are all compared. A projected path must exist on
/// both sides and cannot name the schema or projection identity.
pub fn compare_characterization_captures(
    left_scenario: &ScenarioSpec,
    left: &CharacterizationCapture,
    right_scenario: &ScenarioSpec,
    right: &CharacterizationCapture,
    projection: &CaptureComparisonProjection,
) -> Result<CaptureComparison> {
    if left.projection != projection.identity || right.projection != projection.identity {
        return Err(Error::Eval(format!(
            "capture comparison projection {} is not recorded by both captures",
            projection.identity
        )));
    }
    let left = comparison_datum(left_scenario, left);
    let right = comparison_datum(right_scenario, right);
    for path in &projection.unstable_fields {
        if path == "$@tag" || path == "$.projection" {
            return Err(Error::Eval(format!(
                "capture comparison projection cannot ignore protected field {path}"
            )));
        }
        if !datum_path_exists(&left, "$", path) || !datum_path_exists(&right, "$", path) {
            return Err(Error::Eval(format!(
                "capture comparison projection {} declares non-two-sided field {path}",
                projection.identity
            )));
        }
    }
    let mut differences = Vec::new();
    compare_datum(
        "$",
        &left,
        &right,
        &projection.unstable_fields,
        &mut differences,
    );
    Ok(CaptureComparison {
        projection: projection.identity.clone(),
        differences,
    })
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

fn comparison_datum(scenario: &ScenarioSpec, capture: &CharacterizationCapture) -> Datum {
    let mut datum = capture_datum(scenario, capture);
    let Datum::Node { fields, .. } = &mut datum else {
        unreachable!("capture datum is always a node")
    };
    fields.insert(
        3,
        (
            Symbol::new("selected-lanes"),
            Datum::List(
                scenario
                    .observation_lanes
                    .iter()
                    .map(|lane| Datum::Symbol(observation_lane_symbol(*lane)))
                    .collect(),
            ),
        ),
    );
    datum
}

fn observation_lane_symbol(lane: ScenarioObservationLane) -> Symbol {
    let name = match lane {
        ScenarioObservationLane::ValueOrFailure => "value-or-failure",
        ScenarioObservationLane::Events => "events",
        ScenarioObservationLane::Receipts => "receipts",
        ScenarioObservationLane::Browse => "browse",
    };
    Symbol::qualified("standard/characterization-lane", name)
}

fn datum_path_exists(datum: &Datum, path: &str, wanted: &str) -> bool {
    if path == wanted {
        return true;
    }
    match datum {
        Datum::Node { fields, .. } => {
            wanted == format!("{path}@tag")
                || fields.iter().any(|(name, value)| {
                    datum_path_exists(value, &format!("{path}.{name}"), wanted)
                })
        }
        Datum::List(items) | Datum::Vector(items) | Datum::Set(items) => items
            .iter()
            .enumerate()
            .any(|(index, value)| datum_path_exists(value, &format!("{path}[{index}]"), wanted)),
        Datum::Map(entries) => entries.iter().enumerate().any(|(index, (key, value))| {
            datum_path_exists(key, &format!("{path}.keys[{index}]"), wanted)
                || datum_path_exists(value, &format!("{path}.values[{index}]"), wanted)
        }),
        _ => false,
    }
}

fn compare_datum(
    path: &str,
    left: &Datum,
    right: &Datum,
    ignored: &BTreeSet<String>,
    differences: &mut Vec<CaptureDifference>,
) {
    if ignored.contains(path) || left == right {
        return;
    }
    match (left, right) {
        (
            Datum::Node {
                tag: left_tag,
                fields: left_fields,
            },
            Datum::Node {
                tag: right_tag,
                fields: right_fields,
            },
        ) if left_fields
            .iter()
            .map(|(name, _)| name)
            .eq(right_fields.iter().map(|(name, _)| name)) =>
        {
            if left_tag != right_tag {
                push_capture_difference(
                    format!("{path}@tag"),
                    Datum::Symbol(left_tag.clone()),
                    Datum::Symbol(right_tag.clone()),
                    ignored,
                    differences,
                );
            }
            for ((name, left), (_, right)) in left_fields.iter().zip(right_fields) {
                compare_datum(&format!("{path}.{name}"), left, right, ignored, differences);
            }
        }
        (Datum::List(left), Datum::List(right)) | (Datum::Vector(left), Datum::Vector(right)) => {
            let absent = absent_datum();
            for index in 0..left.len().max(right.len()) {
                compare_datum(
                    &format!("{path}[{index}]"),
                    left.get(index).unwrap_or(&absent),
                    right.get(index).unwrap_or(&absent),
                    ignored,
                    differences,
                );
            }
        }
        (Datum::Set(left), Datum::Set(right)) => {
            let left = canonical_items(left);
            let right = canonical_items(right);
            let absent = absent_datum();
            for index in 0..left.len().max(right.len()) {
                compare_datum(
                    &format!("{path}[{index}]"),
                    left.get(index).copied().unwrap_or(&absent),
                    right.get(index).copied().unwrap_or(&absent),
                    ignored,
                    differences,
                );
            }
        }
        (Datum::Map(left), Datum::Map(right)) => {
            let left = canonical_entries(left);
            let right = canonical_entries(right);
            let absent = absent_datum();
            for index in 0..left.len().max(right.len()) {
                let left = left.get(index).copied();
                let right = right.get(index).copied();
                compare_datum(
                    &format!("{path}.keys[{index}]"),
                    left.map_or(&absent, |entry| &entry.0),
                    right.map_or(&absent, |entry| &entry.0),
                    ignored,
                    differences,
                );
                compare_datum(
                    &format!("{path}.values[{index}]"),
                    left.map_or(&absent, |entry| &entry.1),
                    right.map_or(&absent, |entry| &entry.1),
                    ignored,
                    differences,
                );
            }
        }
        _ => push_capture_difference(
            path.to_owned(),
            left.clone(),
            right.clone(),
            ignored,
            differences,
        ),
    }
}

fn push_capture_difference(
    path: String,
    left: Datum,
    right: Datum,
    ignored: &BTreeSet<String>,
    differences: &mut Vec<CaptureDifference>,
) {
    if !ignored.contains(&path) {
        differences.push(CaptureDifference { path, left, right });
    }
}

fn absent_datum() -> Datum {
    Datum::Node {
        tag: Symbol::qualified("standard/capture-diff", "absent"),
        fields: Vec::new(),
    }
}

fn canonical_items(items: &[Datum]) -> Vec<&Datum> {
    let mut items = items.iter().collect::<Vec<_>>();
    items.sort_by_cached_key(|item| item.canonical_bytes().unwrap_or_default());
    items
}

fn canonical_entries(entries: &[(Datum, Datum)]) -> Vec<&(Datum, Datum)> {
    let mut entries = entries.iter().collect::<Vec<_>>();
    entries.sort_by_cached_key(|(key, _)| key.canonical_bytes().unwrap_or_default());
    entries
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
