/// Read-only, non-resolving view of a JVM class-loader namespace.
///
/// Dependency capacity is allocated once by [`Self::new`]. Queries only inspect
/// already-loaded definitions, append within that capacity, and never enter
/// class initialization, execution, native dispatch, source access, or ordinary
/// symbolic resolution.
pub struct VerificationEnvironment<'a> {
    loader: &'a ClassLoader,
    dependencies: RefCell<Vec<VerificationDependency>>,
    dependency_limit: usize,
}
impl<'a> VerificationEnvironment<'a> {
    /// Creates a view with a fixed proof-dependency allowance.
    pub fn new(loader: &'a ClassLoader, dependency_limit: usize) -> Self {
        Self {
            loader,
            dependencies: RefCell::new(Vec::with_capacity(dependency_limit)),
            dependency_limit,
        }
    }

    /// Defining loader namespace observed by this environment.
    pub fn loader(&self) -> ClassLoaderId {
        self.loader.id()
    }

    /// Exact, deduplicated dependencies accumulated by successful queries.
    pub fn dependencies(&self) -> impl std::ops::Deref<Target = [VerificationDependency]> + '_ {
        std::cell::Ref::map(self.dependencies.borrow(), Vec::as_slice)
    }

    /// Observes one already-loaded class without resolving or initializing it.
    pub fn class(&self, binary_name: &str) -> Result<VerificationClass, VerificationQueryError> {
        self.observe(binary_name)
    }

    /// Checks assignability through already-loaded declared superclass and
    /// interface metadata, charging at most `node_limit` consulted classes.
    pub fn is_assignable(
        &self,
        actual: &str,
        expected: &str,
        node_limit: usize,
    ) -> Result<VerificationAssignability, VerificationQueryError> {
        let mut remaining = node_limit;
        if self.lineage_reaches(actual, expected, node_limit, &mut remaining)? {
            Ok(VerificationAssignability::Assignable)
        } else {
            Ok(VerificationAssignability::NotAssignable)
        }
    }

    /// Checks verifier reference assignability using JVMS 4.10.1.2 rules.
    pub fn reference_assignability(
        &self,
        actual: &ReferenceType,
        expected: &ReferenceType,
        node_limit: usize,
    ) -> Result<VerificationQuery<VerificationAssignability>, VerificationQueryFailure> {
        let start = self.dependencies.borrow().len();
        let mut remaining = node_limit;
        let answer = self.reference_reaches(actual, expected, node_limit, &mut remaining);
        let value =
            if answer.map_err(|error| self.query_failure(error, start, node_limit, remaining))? {
                VerificationAssignability::Assignable
            } else {
                VerificationAssignability::NotAssignable
            };
        Ok(VerificationQuery {
            value,
            evidence: self.query_evidence(start, node_limit, remaining),
        })
    }

    /// Joins two verifier values without resolving missing hierarchy metadata.
    pub fn join_types(
        &self,
        left: &VerificationType,
        right: &VerificationType,
        node_limit: usize,
    ) -> Result<VerificationQuery<VerificationTypeJoin>, VerificationQueryFailure> {
        let start = self.dependencies.borrow().len();
        let mut remaining = node_limit;
        let joined = self
            .join_types_inner(left, right, node_limit, &mut remaining)
            .map_err(|error| self.query_failure(error, start, node_limit, remaining))?;
        Ok(VerificationQuery {
            value: joined,
            evidence: self.query_evidence(start, node_limit, remaining),
        })
    }

    fn query_evidence(
        &self,
        start: usize,
        limit: usize,
        remaining: usize,
    ) -> VerificationQueryEvidence {
        VerificationQueryEvidence {
            dependencies: self.dependencies.borrow()[start..]
                .iter()
                .map(|d| d.class().clone())
                .collect(),
            node_limit: limit,
            nodes_used: limit - remaining,
        }
    }

    fn query_failure(
        &self,
        error: VerificationQueryError,
        start: usize,
        limit: usize,
        remaining: usize,
    ) -> VerificationQueryFailure {
        VerificationQueryFailure {
            error,
            evidence: self.query_evidence(start, limit, remaining),
        }
    }

    fn reference_reaches(
        &self,
        actual: &ReferenceType,
        expected: &ReferenceType,
        limit: usize,
        remaining: &mut usize,
    ) -> Result<bool, VerificationQueryError> {
        if actual == expected || matches!(expected, ReferenceType::Object) {
            return Ok(true);
        }
        match (actual, expected) {
            (ReferenceType::Class(actual), ReferenceType::Class(expected)) => {
                self.lineage_reaches(actual, expected, limit, remaining)
            }
            (ReferenceType::Array(_), ReferenceType::Class(expected))
                if matches!(
                    expected.as_ref(),
                    "java/lang/Cloneable" | "java/io/Serializable"
                ) =>
            {
                Ok(true)
            }
            (ReferenceType::Array(actual), ReferenceType::Array(expected)) => {
                self.array_assignable(actual, expected, limit, remaining)
            }
            _ => Ok(false),
        }
    }

    fn array_assignable(
        &self,
        actual: &str,
        expected: &str,
        limit: usize,
        remaining: &mut usize,
    ) -> Result<bool, VerificationQueryError> {
        let (Some(a), Some(e)) = (actual.strip_prefix('['), expected.strip_prefix('[')) else {
            return Ok(false);
        };
        if is_primitive_descriptor(a) || is_primitive_descriptor(e) {
            return Ok(a == e);
        }
        self.reference_reaches(
            &descriptor_reference(a)?,
            &descriptor_reference(e)?,
            limit,
            remaining,
        )
    }

    fn join_types_inner(
        &self,
        left: &VerificationType,
        right: &VerificationType,
        limit: usize,
        remaining: &mut usize,
    ) -> Result<VerificationTypeJoin, VerificationQueryError> {
        use VerificationType::{Bottom, Null, Reference, Unusable};
        let plain = |value| VerificationTypeJoin { value, rule: None };
        match (left, right) {
            (Bottom, value) | (value, Bottom) => Ok(plain(value.clone())),
            (Unusable, _) | (_, Unusable) => Ok(plain(Unusable)),
            (a, b) if a == b => Ok(plain(a.clone())),
            (Null, Reference(r)) | (Reference(r), Null) => Ok(plain(Reference(r.clone()))),
            (Reference(a), Reference(b)) => self.join_references(a, b, limit, remaining),
            _ => Ok(plain(Unusable)),
        }
    }

    fn join_references(
        &self,
        left: &ReferenceType,
        right: &ReferenceType,
        limit: usize,
        remaining: &mut usize,
    ) -> Result<VerificationTypeJoin, VerificationQueryError> {
        let result = |r, rule| VerificationTypeJoin {
            value: VerificationType::Reference(r),
            rule: Some(rule),
        };
        if self.reference_reaches(left, right, limit, remaining)? {
            return Ok(result(right.clone(), VerificationJoinRule::AssignableInput));
        }
        if self.reference_reaches(right, left, limit, remaining)? {
            return Ok(result(left.clone(), VerificationJoinRule::AssignableInput));
        }
        if let (ReferenceType::Array(a), ReferenceType::Array(b)) = (left, right) {
            let (Some(ac), Some(bc)) = (a.strip_prefix('['), b.strip_prefix('[')) else {
                unreachable!()
            };
            if !is_primitive_descriptor(ac) && !is_primitive_descriptor(bc) {
                let joined = self.join_references(
                    &descriptor_reference(ac)?,
                    &descriptor_reference(bc)?,
                    limit,
                    remaining,
                )?;
                if let VerificationType::Reference(reference) = joined.value {
                    return Ok(result(
                        ReferenceType::Array(
                            format!("[{}", reference_descriptor(&reference)).into_boxed_str(),
                        ),
                        VerificationJoinRule::ArrayComponents,
                    ));
                }
            }
        }
        let (ReferenceType::Class(a), ReferenceType::Class(b)) = (left, right) else {
            return Ok(result(
                ReferenceType::Object,
                VerificationJoinRule::CommonSuperclass,
            ));
        };
        let ac = self.observe(a)?;
        let bc = self.observe(b)?;
        if ac.is_interface() && bc.is_interface() {
            return Ok(result(
                ReferenceType::Object,
                VerificationJoinRule::UnrelatedInterfaces,
            ));
        }
        let mut current = a.to_string();
        loop {
            if self.lineage_reaches(b, &current, limit, remaining)? {
                return Ok(result(
                    ReferenceType::Class(current.clone().into_boxed_str()),
                    VerificationJoinRule::CommonSuperclass,
                ));
            }
            if *remaining == 0 {
                return Err(VerificationQueryError::LineageLimit { limit });
            }
            *remaining -= 1;
            let class = self.observe(&current)?;
            let Some(parent) = class.metadata().resolution().direct_parents().first() else {
                break;
            };
            current.clone_from(parent);
        }
        Ok(result(
            ReferenceType::Object,
            VerificationJoinRule::CommonSuperclass,
        ))
    }

    fn lineage_reaches(
        &self,
        binary_name: &str,
        expected: &str,
        limit: usize,
        remaining: &mut usize,
    ) -> Result<bool, VerificationQueryError> {
        if *remaining == 0 {
            return Err(VerificationQueryError::LineageLimit { limit });
        }
        *remaining -= 1;
        let class = self.observe(binary_name)?;
        if class.id().binary_name() == expected {
            return Ok(true);
        }
        for parent in class.metadata().resolution().direct_parents() {
            if self.lineage_reaches(parent, expected, limit, remaining)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn observe(&self, binary_name: &str) -> Result<VerificationClass, VerificationQueryError> {
        let before = self.loader.revision();
        let definition = self
            .loader
            .loaded(binary_name)
            .map_err(|_| VerificationQueryError::NotLoaded(binary_name.to_owned()))?
            .ok_or_else(|| VerificationQueryError::NotLoaded(binary_name.to_owned()))?;
        let after = self.loader.revision();
        if before != after {
            return Err(VerificationQueryError::ConcurrentRevision { before, after });
        }
        let mut dependencies = self.dependencies.borrow_mut();
        if !dependencies
            .iter()
            .any(|dependency| dependency.class.id() == definition.id())
        {
            if dependencies.len() == self.dependency_limit {
                return Err(VerificationQueryError::DependencyLimit {
                    limit: self.dependency_limit,
                });
            }
            dependencies.push(VerificationDependency {
                class: definition.clone(),
                revision: after,
            });
        }
        drop(dependencies);
        Ok(VerificationClass { definition })
    }
}
