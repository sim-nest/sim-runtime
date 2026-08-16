//! Exact Java text, literal interning, and core class mirrors.

use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex, MutexGuard, Weak},
};

use sim_kernel::{Error, Result};
use sim_text::{CodeUnitOffset, CodeUnitRange, CodeUnitString};

/// Immutable managed `java.lang.String` value.
///
/// Identity is the allocation, while Java equality and hashing use the exact
/// UTF-16 code units. No scalar-Unicode conversion occurs in a value position.
#[derive(Clone, Debug)]
pub struct JavaString(Arc<CodeUnitString>);

impl JavaString {
    /// Allocates a non-interned string over exact UTF-16 code units.
    pub fn new(storage: CodeUnitString) -> Self {
        Self(Arc::new(storage))
    }

    /// Returns the canonical shared code-unit storage.
    pub fn storage(&self) -> &CodeUnitString {
        &self.0
    }

    /// Tests Java string contents without Unicode normalization or repair.
    pub fn content_equals(&self, other: &Self) -> bool {
        self.0 == other.0
    }

    /// Returns the Java `String.hashCode()` value over raw UTF-16 units.
    pub fn java_hash(&self) -> i32 {
        self.0.code_units().fold(0_i32, |hash, unit| {
            hash.wrapping_mul(31).wrapping_add(i32::from(unit))
        })
    }

    /// Tests managed identity (the operation used by Java `==`).
    pub fn identical(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Concatenates exact code units into a fresh managed string.
    pub fn concat(&self, other: &Self) -> Result<Self> {
        let mut units = Vec::new();
        units
            .try_reserve(self.0.len().saturating_add(other.0.len()))
            .map_err(|_| Error::Eval("Java string concatenation allocation failed".into()))?;
        units.extend(self.0.code_units());
        units.extend(other.0.code_units());
        CodeUnitString::try_from_code_units(units)
            .map(Self::new)
            .map_err(|error| Error::Eval(error.to_string()))
    }

    /// Slices using Java's UTF-16 code-unit indexing policy.
    pub fn substring(&self, start: usize, end: usize) -> Result<Self> {
        if start > end || end > self.0.len() {
            return Err(Error::Eval(format!(
                "Java substring range {start}..{end} exceeds length {}",
                self.0.len()
            )));
        }
        Ok(Self::new(self.0.slice(CodeUnitRange::new(
            CodeUnitOffset::new(start),
            CodeUnitOffset::new(end),
        ))))
    }
}

impl PartialEq for JavaString {
    fn eq(&self, other: &Self) -> bool {
        self.content_equals(other)
    }
}
impl Eq for JavaString {}
impl Hash for JavaString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

/// Loader-owned, count-bounded canonical literal table.
pub(crate) struct JavaInternPool {
    max_entries: usize,
    entries: Mutex<HashMap<CodeUnitString, Weak<CodeUnitString>>>,
}

impl JavaInternPool {
    pub(crate) fn new(max_entries: usize) -> Self {
        Self {
            max_entries,
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn entries(&self) -> Result<MutexGuard<'_, HashMap<CodeUnitString, Weak<CodeUnitString>>>> {
        self.entries
            .lock()
            .map_err(|_| Error::Eval("JVM intern pool lock poisoned".into()))
    }

    pub(crate) fn intern(&self, units: &CodeUnitString) -> Result<JavaString> {
        let mut entries = self.entries()?;
        if let Some(existing) = entries.get(units).and_then(Weak::upgrade) {
            return Ok(JavaString(existing));
        }
        entries.retain(|_, value| value.strong_count() != 0);
        if entries.len() >= self.max_entries {
            return Err(Error::Eval(format!(
                "interned string allowance of {} exhausted",
                self.max_entries
            )));
        }
        let storage = Arc::new(units.clone());
        entries.insert(units.clone(), Arc::downgrade(&storage));
        Ok(JavaString(storage))
    }
}

/// Admitted core member implemented by the JVM profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JavaCoreMember {
    /// `java.lang.Object.equals(Object)`.
    ObjectEquals,
    /// `java.lang.Object.hashCode()`.
    ObjectHashCode,
    /// `java.lang.Object.getClass()`.
    ObjectGetClass,
    /// `java.lang.String.equals(Object)`.
    StringEquals,
    /// `java.lang.String.hashCode()`.
    StringHashCode,
    /// `java.lang.String.length()`.
    StringLength,
    /// `java.lang.String.charAt(int)`.
    StringCharAt,
    /// `java.lang.String.substring(int,int)`.
    StringSubstring,
    /// `java.lang.String.concat(String)`.
    StringConcat,
    /// `java.lang.String.intern()`.
    StringIntern,
    /// `java.lang.Class.getName()`.
    ClassGetName,
    /// `java.lang.Class.getClassLoader()`.
    ClassGetClassLoader,
}

/// Java-visible mirror of one loaded class identity.
#[derive(Clone, Debug)]
pub struct JavaClassMirror {
    definition: Arc<crate::ClassDefinition>,
}

impl JavaClassMirror {
    pub(crate) fn new(definition: Arc<crate::ClassDefinition>) -> Self {
        Self { definition }
    }

    /// The mirrored definition, including its defining loader identity.
    pub fn definition(&self) -> &Arc<crate::ClassDefinition> {
        &self.definition
    }

    /// Tests Java class-mirror identity.
    pub fn identical(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.definition, &other.definition)
    }
}

/// Complete deliberately admitted `Object`, `String`, and `Class` member set.
pub const ADMITTED_CORE_MEMBERS: &[JavaCoreMember] = &[
    JavaCoreMember::ObjectEquals,
    JavaCoreMember::ObjectHashCode,
    JavaCoreMember::ObjectGetClass,
    JavaCoreMember::StringEquals,
    JavaCoreMember::StringHashCode,
    JavaCoreMember::StringLength,
    JavaCoreMember::StringCharAt,
    JavaCoreMember::StringSubstring,
    JavaCoreMember::StringConcat,
    JavaCoreMember::StringIntern,
    JavaCoreMember::ClassGetName,
    JavaCoreMember::ClassGetClassLoader,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lone_surrogate_survives_intern_compare_concat_substring_and_browse() {
        let pool = JavaInternPool::new(2);
        let exact = CodeUnitString::from_code_units(vec![b'a'.into(), 0xd800]);
        let first = pool.intern(&exact).unwrap();
        let second = pool.intern(&exact.clone()).unwrap();
        assert!(first.identical(&second));
        assert!(first.content_equals(&second));
        assert_eq!(first.java_hash(), 0xe3bf);

        let suffix = JavaString::new(CodeUnitString::from_code_units(vec![b'z'.into()]));
        let joined = first.concat(&suffix).unwrap();
        assert_eq!(joined.storage().as_code_units(), &[0x61, 0xd800, 0x7a]);
        assert_eq!(
            joined.substring(1, 2).unwrap().storage().as_code_units(),
            &[0xd800]
        );
    }

    #[test]
    fn intern_pool_is_bounded_but_reclaims_unreferenced_entries() {
        let pool = JavaInternPool::new(1);
        let retained = pool
            .intern(&CodeUnitString::from_code_units(vec![1]))
            .unwrap();
        assert!(
            pool.intern(&CodeUnitString::from_code_units(vec![2]))
                .is_err()
        );
        drop(retained);
        assert!(
            pool.intern(&CodeUnitString::from_code_units(vec![2]))
                .is_ok()
        );
    }
}
