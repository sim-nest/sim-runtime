//! UTF-16 code-unit face for ECMAScript strings.

use sim_text::{CodeUnitOffset, CodeUnitRange, CodeUnitString, CodeUnitStringError};

/// Error crossing from the code-unit face to canonical scalar SIM text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JavascriptTextError {
    /// The unit sequence contains an unpaired surrogate.
    LoneSurrogate {
        /// Code-unit index of the first invalid surrogate.
        index: usize,
        /// Invalid code unit.
        unit: u16,
    },
}

/// An ECMAScript String as exact UTF-16 code units.
///
/// This face admits lone surrogates. Canonical SIM text remains scalar Unicode;
/// conversion to it is explicit and fails rather than replacing data.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct JavascriptCodeUnitString(CodeUnitString);
impl JavascriptCodeUnitString {
    /// Encode canonical scalar text into the JavaScript face.
    pub fn from_scalar(text: &str) -> Self {
        Self(CodeUnitString::from_scalar(text))
    }
    /// Preserve an exact sequence including lone surrogates.
    pub fn from_code_units(units: Vec<u16>) -> Self {
        Self(CodeUnitString::from_code_units(units))
    }
    /// ECMAScript `length` in code units.
    pub fn len(&self) -> usize {
        self.0.len()
    }
    /// Whether the string has no code units.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// Index one code unit.
    pub fn code_unit_at(&self, index: usize) -> Option<u16> {
        self.0.code_unit_at(CodeUnitOffset::new(index))
    }
    /// Slice by code-unit indices, clamped as `String.prototype.slice` does for nonnegative indices.
    pub fn slice(&self, start: usize, end: usize) -> Self {
        Self(self.0.slice(CodeUnitRange::new(
            CodeUnitOffset::new(start),
            CodeUnitOffset::new(end),
        )))
    }
    /// Iterate exact code units (the indexing face).
    pub fn code_units(&self) -> impl Iterator<Item = u16> + '_ {
        self.0.code_units()
    }
    /// Iterate ECMAScript string iterator chunks: paired surrogates together, lone units alone.
    pub fn iter_strings(&self) -> JavascriptStringIterator<'_> {
        JavascriptStringIterator(self.0.iter_code_points())
    }
    /// Convert only well-formed UTF-16 to canonical scalar SIM text.
    pub fn to_scalar(&self) -> Result<String, JavascriptTextError> {
        self.0.to_scalar().map_err(|error| match error {
            CodeUnitStringError::LoneSurrogate(invalid) => JavascriptTextError::LoneSurrogate {
                index: invalid.offset.get(),
                unit: invalid.unit,
            },
            CodeUnitStringError::TooLong { .. } => {
                unreachable!("an existing code-unit string already satisfies the allocation limit")
            }
        })
    }
}
/// Iterator over ECMAScript code-point chunks represented as exact code-unit strings.
pub struct JavascriptStringIterator<'a>(sim_text::CodePointIter<'a>);
impl Iterator for JavascriptStringIterator<'_> {
    type Item = JavascriptCodeUnitString;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(JavascriptCodeUnitString)
    }
}

#[cfg(test)]
mod law_fixtures {
    use super::*;
    use std::sync::Arc;

    use sim_kernel::{Cx, Datum, DefaultFactory, NoopEvalPolicy, Ref, Symbol};
    use sim_lib_standard_core::{
        BoundedLane, CanonicalObservation, CanonicalOutcome, CharacterizationCapture,
        ScenarioLimits, ScenarioObservationLane, ScenarioSpec, publish_characterization_capture,
    };

    fn units_datum(units: impl IntoIterator<Item = u16>) -> Datum {
        Datum::Vector(
            units
                .into_iter()
                .map(|unit| Datum::String(format!("{unit:04x}")))
                .collect(),
        )
    }

    fn characterized_case(name: &str, value: JavascriptCodeUnitString) -> Datum {
        Datum::Node {
            tag: Symbol::qualified("javascript-utf16", "case/v1"),
            fields: vec![
                (Symbol::new("name"), Datum::String(name.to_owned())),
                (
                    Symbol::new("length"),
                    Datum::String(value.len().to_string()),
                ),
                (Symbol::new("code-units"), units_datum(value.code_units())),
                (
                    Symbol::new("string-iterator"),
                    Datum::Vector(
                        value
                            .iter_strings()
                            .map(|chunk| units_datum(chunk.code_units()))
                            .collect(),
                    ),
                ),
            ],
        }
    }

    fn characterization_capture() -> (ScenarioSpec, CharacterizationCapture) {
        let astral = JavascriptCodeUnitString::from_scalar("😀");
        let cases = vec![
            characterized_case("empty", JavascriptCodeUnitString::default()),
            characterized_case("astral-pair", astral.clone()),
            characterized_case(
                "lone-high-start",
                JavascriptCodeUnitString::from_code_units(vec![0xd800, 0x0061]),
            ),
            characterized_case(
                "lone-high-middle",
                JavascriptCodeUnitString::from_code_units(vec![0x0061, 0xd800, 0x0062]),
            ),
            characterized_case(
                "lone-high-end",
                JavascriptCodeUnitString::from_code_units(vec![0x0061, 0xd800]),
            ),
            characterized_case(
                "lone-low",
                JavascriptCodeUnitString::from_code_units(vec![0xdc00]),
            ),
            characterized_case("nul", JavascriptCodeUnitString::from_scalar("\0")),
            characterized_case("slice-across-pair-high", astral.slice(0, 1)),
            characterized_case("slice-across-pair-low", astral.slice(1, 2)),
        ];
        let scenario = ScenarioSpec::new(
            Symbol::qualified("javascript", "characterize-utf16/v1"),
            Symbol::qualified("javascript", "text-current/v1"),
        )
        .with_limits(ScenarioLimits::new(0, 1))
        .observing(ScenarioObservationLane::ValueOrFailure);
        let capture = CharacterizationCapture::new(
            Symbol::qualified("javascript", "utf16-code-units/v1"),
            CanonicalObservation {
                outcome: Some(CanonicalOutcome::Success(Datum::Vector(cases))),
                events: BoundedLane::Absent,
                receipts: BoundedLane::Absent,
                browse: BoundedLane::Absent,
            },
        );
        (scenario, capture)
    }

    fn test_cx() -> Cx {
        Cx::new(Arc::new(NoopEvalPolicy), Arc::new(DefaultFactory))
    }

    #[test]
    fn characterize_1_current_code_unit_behavior_has_stable_content_identity() {
        let (scenario, capture) = characterization_capture();
        let first = publish_characterization_capture(&mut test_cx(), &scenario, &capture).unwrap();
        let second = publish_characterization_capture(&mut test_cx(), &scenario, &capture).unwrap();
        assert!(matches!(first, Ref::Content(_)));
        assert_eq!(first, second);
    }

    #[test]
    fn length_index_slice_and_code_unit_iteration_are_utf16() {
        let s = JavascriptCodeUnitString::from_scalar("A😀B");
        assert_eq!(s.len(), 4);
        assert_eq!(s.code_unit_at(1), Some(0xd83d));
        assert_eq!(
            s.slice(1, 3).code_units().collect::<Vec<_>>(),
            vec![0xd83d, 0xde00]
        );
        assert_eq!(s.code_units().count(), 4);
    }
    #[test]
    fn scalar_conversion_and_paired_iteration_are_exact() {
        let s = JavascriptCodeUnitString::from_code_units(vec![0xd83d, 0xde00]);
        assert_eq!(s.to_scalar().unwrap(), "😀");
        assert_eq!(s.iter_strings().next().unwrap().len(), 2);
    }
    #[test]
    fn lone_high_surrogate_is_preserved_and_rejected_by_scalar_face() {
        let s = JavascriptCodeUnitString::from_code_units(vec![0xd800]);
        assert_eq!(s.code_unit_at(0), Some(0xd800));
        assert_eq!(
            s.iter_strings()
                .next()
                .unwrap()
                .code_units()
                .collect::<Vec<_>>(),
            vec![0xd800]
        );
        assert_eq!(
            s.to_scalar(),
            Err(JavascriptTextError::LoneSurrogate {
                index: 0,
                unit: 0xd800
            })
        );
    }
    #[test]
    fn lone_low_surrogate_is_preserved_and_rejected_by_scalar_face() {
        let s = JavascriptCodeUnitString::from_code_units(vec![0xdc00]);
        assert_eq!(s.slice(0, 1), s);
        assert_eq!(
            s.to_scalar(),
            Err(JavascriptTextError::LoneSurrogate {
                index: 0,
                unit: 0xdc00
            })
        );
    }

    #[test]
    fn scalar_error_preserves_javascript_code_unit_position() {
        let s = JavascriptCodeUnitString::from_code_units(vec![0x0061, 0xd800, 0x0062]);
        assert_eq!(
            s.to_scalar(),
            Err(JavascriptTextError::LoneSurrogate {
                index: 1,
                unit: 0xd800,
            })
        );
    }
}
