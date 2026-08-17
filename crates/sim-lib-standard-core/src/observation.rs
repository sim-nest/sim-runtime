//! Canonical, bounded projections of characterization observations.

use sim_kernel::{Datum, Error, Result, Symbol};

/// Profile-owned semantic projection for a guest value.
pub type GuestValueProjection<T> = dyn Fn(&T) -> Result<Datum>;

/// A bounded observation lane, retaining the distinction between no lane,
/// an observed empty lane, and an observed prefix whose tail was omitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoundedLane<T> {
    /// The scenario did not select this lane.
    Absent,
    /// The complete ordered lane, including an observed empty lane.
    Complete(Vec<T>),
    /// The ordered prefix retained when the lane exceeded its bound.
    Truncated {
        /// Items retained in their original order.
        items: Vec<T>,
        /// Exact number of omitted trailing items.
        omitted: usize,
    },
}

impl<T> BoundedLane<T> {
    /// Capture an explicitly selected lane under `limit`.
    pub fn capture(items: Vec<T>, limit: usize) -> Self {
        if items.len() <= limit {
            Self::Complete(items)
        } else {
            let omitted = items.len() - limit;
            Self::Truncated {
                items: items.into_iter().take(limit).collect(),
                omitted,
            }
        }
    }
}

/// Stable source location attached to a failed outcome.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailureLocation {
    /// Stable source identity, independent of a host filesystem path.
    pub source: Symbol,
    /// Zero-based start byte in the identified source.
    pub start: usize,
    /// Zero-based exclusive end byte in the identified source.
    pub end: usize,
}

/// Canonical failure fields used by characterization captures.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalFailure {
    /// Stable failure class.
    pub class: Symbol,
    /// Semantic failure detail; display strings are not accepted here.
    pub detail: Datum,
    /// Optional stable source location.
    pub location: Option<FailureLocation>,
}

/// Canonical result of a scenario.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanonicalOutcome {
    /// Successful semantic data.
    Success(Datum),
    /// Stable failure data.
    Failure(CanonicalFailure),
}

/// Every observable scenario lane after semantic projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalObservation {
    /// Selected value-or-failure lane, or `None` when absent.
    pub outcome: Option<CanonicalOutcome>,
    /// Ordered event data.
    pub events: BoundedLane<Datum>,
    /// Ordered receipt data.
    pub receipts: BoundedLane<Datum>,
    /// Ordered browse/Card data.
    pub browse: BoundedLane<Datum>,
}

/// Project a guest value that has no intrinsic canonical data face.
///
/// The caller must supply a profile-owned semantic projection. Deliberately
/// there is no `Debug` or display fallback.
pub fn project_guest_value<T>(
    value: &T,
    projection: Option<&GuestValueProjection<T>>,
) -> Result<Datum> {
    projection.ok_or_else(|| {
        Error::Eval("guest value has no canonical data face or profile projection".to_owned())
    })?(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct HostValue {
        semantic: i64,
        host_format: &'static str,
    }

    fn semantic_projection(value: &HostValue) -> Result<Datum> {
        let _host_only_formatting = value.host_format;
        Ok(Datum::String(value.semantic.to_string()))
    }

    fn observation(receipts: Vec<Datum>, location: FailureLocation) -> CanonicalObservation {
        CanonicalObservation {
            outcome: Some(CanonicalOutcome::Failure(CanonicalFailure {
                class: Symbol::qualified("test", "rejected"),
                detail: Datum::String("invalid-input".to_owned()),
                location: Some(location),
            })),
            events: BoundedLane::Complete(vec![Datum::String("started".to_owned())]),
            receipts: BoundedLane::capture(receipts, 4),
            browse: BoundedLane::Complete(Vec::new()),
        }
    }

    #[test]
    fn guest_projection_ignores_host_formatting_and_is_mandatory() {
        let terse = HostValue {
            semantic: 7,
            host_format: "7",
        };
        let verbose = HostValue {
            semantic: 7,
            host_format: "HostValue(7)",
        };

        assert_eq!(
            project_guest_value(&terse, Some(&semantic_projection)).unwrap(),
            project_guest_value(&verbose, Some(&semantic_projection)).unwrap()
        );
        assert!(project_guest_value(&terse, None).is_err());
    }

    #[test]
    fn receipt_order_and_failure_location_are_semantic() {
        let location = FailureLocation {
            source: Symbol::qualified("fixture", "source"),
            start: 2,
            end: 5,
        };
        let first = Datum::String("first".to_owned());
        let second = Datum::String("second".to_owned());
        let baseline = observation(vec![first.clone(), second.clone()], location.clone());

        assert_ne!(baseline, observation(vec![second, first], location.clone()));
        assert_ne!(
            baseline,
            observation(
                vec![
                    Datum::String("first".to_owned()),
                    Datum::String("second".to_owned())
                ],
                FailureLocation {
                    start: 3,
                    ..location
                }
            )
        );
    }

    #[test]
    fn absent_empty_and_truncated_lanes_remain_distinct() {
        let empty = BoundedLane::<Datum>::capture(Vec::new(), 1);
        let truncated = BoundedLane::capture(vec![Datum::Bool(true), Datum::Bool(false)], 1);

        assert_ne!(BoundedLane::Absent, empty);
        assert_ne!(empty, truncated);
        assert_eq!(
            truncated,
            BoundedLane::Truncated {
                items: vec![Datum::Bool(true)],
                omitted: 1,
            }
        );
    }
}
