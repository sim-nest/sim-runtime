//! UTF-16 code-unit face for ECMAScript strings.

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
pub struct JavascriptCodeUnitString {
    units: Vec<u16>,
}
impl JavascriptCodeUnitString {
    /// Encode canonical scalar text into the JavaScript face.
    pub fn from_scalar(text: &str) -> Self {
        Self {
            units: text.encode_utf16().collect(),
        }
    }
    /// Preserve an exact sequence including lone surrogates.
    pub fn from_code_units(units: Vec<u16>) -> Self {
        Self { units }
    }
    /// ECMAScript `length` in code units.
    pub fn len(&self) -> usize {
        self.units.len()
    }
    /// Whether the string has no code units.
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }
    /// Index one code unit.
    pub fn code_unit_at(&self, index: usize) -> Option<u16> {
        self.units.get(index).copied()
    }
    /// Slice by code-unit indices, clamped as `String.prototype.slice` does for nonnegative indices.
    pub fn slice(&self, start: usize, end: usize) -> Self {
        let start = start.min(self.len());
        let end = end.max(start).min(self.len());
        Self::from_code_units(self.units[start..end].to_vec())
    }
    /// Iterate exact code units (the indexing face).
    pub fn code_units(&self) -> impl Iterator<Item = u16> + '_ {
        self.units.iter().copied()
    }
    /// Iterate ECMAScript string iterator chunks: paired surrogates together, lone units alone.
    pub fn iter_strings(&self) -> JavascriptStringIterator<'_> {
        JavascriptStringIterator {
            units: &self.units,
            at: 0,
        }
    }
    /// Convert only well-formed UTF-16 to canonical scalar SIM text.
    pub fn to_scalar(&self) -> Result<String, JavascriptTextError> {
        String::from_utf16(&self.units).map_err(|_| {
            let (index, unit) =
                first_lone(&self.units).expect("invalid UTF-16 has a lone surrogate");
            JavascriptTextError::LoneSurrogate { index, unit }
        })
    }
}
/// Iterator over ECMAScript code-point chunks represented as exact code-unit strings.
pub struct JavascriptStringIterator<'a> {
    units: &'a [u16],
    at: usize,
}
impl Iterator for JavascriptStringIterator<'_> {
    type Item = JavascriptCodeUnitString;
    fn next(&mut self) -> Option<Self::Item> {
        let first = *self.units.get(self.at)?;
        let width = if (0xd800..=0xdbff).contains(&first)
            && self
                .units
                .get(self.at + 1)
                .is_some_and(|u| (0xdc00..=0xdfff).contains(u))
        {
            2
        } else {
            1
        };
        let out = JavascriptCodeUnitString::from_code_units(
            self.units[self.at..self.at + width].to_vec(),
        );
        self.at += width;
        Some(out)
    }
}
fn first_lone(units: &[u16]) -> Option<(usize, u16)> {
    let mut i = 0;
    while i < units.len() {
        let u = units[i];
        if (0xd800..=0xdbff).contains(&u) {
            if units
                .get(i + 1)
                .is_some_and(|v| (0xdc00..=0xdfff).contains(v))
            {
                i += 2;
                continue;
            }
            return Some((i, u));
        }
        if (0xdc00..=0xdfff).contains(&u) {
            return Some((i, u));
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod law_fixtures {
    use super::*;
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
}
