//! Checked semilattice state and transfer-policy admission.

use crate::ValueFingerprint;
use std::{error::Error, fmt};

/// State that can report the memory it contributes to a dataflow solution.
pub trait StateSize {
    /// Returns the state payload's accounted size in bytes.
    fn state_size(&self) -> usize;
}

/// A join semilattice with a least element.
///
/// Implementations are admitted only after [`LawSuite`] checks their observable
/// laws over the representative states supplied by the consumer.
pub trait JoinSemilattice: Clone + Eq + StateSize {
    /// Returns the least element.
    fn bottom(&self) -> Self;

    /// Returns the least upper bound of `self` and `other`.
    fn join(&self, other: &Self) -> Self;

    /// Reports the semilattice partial order.
    fn less_equal(&self, other: &Self) -> bool;
}

/// A deterministic, inflationary dataflow transfer with stable proof identity.
///
/// A policy is deliberately an object rather than a closure: its fingerprint
/// participates in cache and proof identity.
pub trait TransferPolicy<S> {
    /// Stable identity of the policy's semantics and configuration.
    fn fingerprint(&self) -> ValueFingerprint;

    /// Accounts for policy configuration retained by an admitted analysis.
    fn policy_size(&self) -> usize;

    /// Computes the next state.
    fn transfer(&self, state: &S) -> S;
}

/// A law whose failure makes a lattice or transfer policy inadmissible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataflowLaw {
    /// `a join a = a`.
    JoinIdempotent,
    /// `a join b = b join a`.
    JoinCommutative,
    /// `(a join b) join c = a join (b join c)`.
    JoinAssociative,
    /// Bottom is below every state and is the identity of join.
    Bottom,
    /// The declared order agrees with join and is reflexive and antisymmetric.
    PartialOrderConsistent,
    /// Repeated evaluation at one input produces the same result.
    TransferDeterministic,
    /// Transfer preserves ordering between comparable inputs.
    TransferMonotone,
    /// Transfer never retracts facts from its input.
    TransferProgress,
}

/// A precise admission refusal.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LawViolation {
    law: DataflowLaw,
    witnesses: Box<[usize]>,
}

impl LawViolation {
    fn new(law: DataflowLaw, witnesses: impl Into<Box<[usize]>>) -> Self {
        Self {
            law,
            witnesses: witnesses.into(),
        }
    }

    /// Returns the law that was broken.
    #[must_use]
    pub const fn law(&self) -> DataflowLaw {
        self.law
    }

    /// Returns indices into the admission sample set that witness the failure.
    #[must_use]
    pub fn witnesses(&self) -> &[usize] {
        &self.witnesses
    }
}

impl fmt::Display for LawViolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "dataflow admission failed {:?} at sample indices {:?}",
            self.law, self.witnesses
        )
    }
}

impl Error for LawViolation {}

/// Public reusable law suite for lattice and transfer admission.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LawSuite;

impl LawSuite {
    /// Checks all semilattice laws over a non-empty representative state set.
    pub fn check_lattice<S: JoinSemilattice>(samples: &[S]) -> Result<(), LawViolation> {
        let Some(first) = samples.first() else {
            return Err(LawViolation::new(DataflowLaw::Bottom, Vec::new()));
        };
        let bottom = first.bottom();
        for (a_index, a) in samples.iter().enumerate() {
            if a.join(a) != *a {
                return Err(LawViolation::new(DataflowLaw::JoinIdempotent, [a_index]));
            }
            if !bottom.less_equal(a) || bottom.join(a) != *a || a.join(&bottom) != *a {
                return Err(LawViolation::new(DataflowLaw::Bottom, [a_index]));
            }
            if !a.less_equal(a) {
                return Err(LawViolation::new(
                    DataflowLaw::PartialOrderConsistent,
                    [a_index],
                ));
            }
            for (b_index, b) in samples.iter().enumerate() {
                if a.join(b) != b.join(a) {
                    return Err(LawViolation::new(
                        DataflowLaw::JoinCommutative,
                        [a_index, b_index],
                    ));
                }
                let join_order = a.join(b) == *b;
                if a.less_equal(b) != join_order || (a.less_equal(b) && b.less_equal(a) && a != b) {
                    return Err(LawViolation::new(
                        DataflowLaw::PartialOrderConsistent,
                        [a_index, b_index],
                    ));
                }
                for (c_index, c) in samples.iter().enumerate() {
                    if a.join(b).join(c) != a.join(&b.join(c)) {
                        return Err(LawViolation::new(
                            DataflowLaw::JoinAssociative,
                            [a_index, b_index, c_index],
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    /// Checks deterministic, monotone, inflationary transfer over the samples.
    pub fn check_transfer<S, P>(policy: &P, samples: &[S]) -> Result<(), LawViolation>
    where
        S: JoinSemilattice,
        P: TransferPolicy<S>,
    {
        for (a_index, a) in samples.iter().enumerate() {
            let output = policy.transfer(a);
            if output != policy.transfer(a) {
                return Err(LawViolation::new(
                    DataflowLaw::TransferDeterministic,
                    [a_index],
                ));
            }
            if !a.less_equal(&output) {
                return Err(LawViolation::new(DataflowLaw::TransferProgress, [a_index]));
            }
            for (b_index, b) in samples.iter().enumerate() {
                if a.less_equal(b) && !output.less_equal(&policy.transfer(b)) {
                    return Err(LawViolation::new(
                        DataflowLaw::TransferMonotone,
                        [a_index, b_index],
                    ));
                }
            }
        }
        Ok(())
    }
}

/// A transfer policy that passed the public dataflow law suite.
#[derive(Clone, Debug)]
pub struct AdmittedTransfer<P> {
    policy: P,
    fingerprint: ValueFingerprint,
    policy_size: usize,
}

impl<P> AdmittedTransfer<P> {
    /// Admits a policy only when both lattice and transfer laws hold.
    pub fn admit<S>(policy: P, samples: &[S]) -> Result<Self, LawViolation>
    where
        S: JoinSemilattice,
        P: TransferPolicy<S>,
    {
        LawSuite::check_lattice(samples)?;
        LawSuite::check_transfer(&policy, samples)?;
        let fingerprint = policy.fingerprint();
        let policy_size = policy.policy_size();
        Ok(Self {
            policy,
            fingerprint,
            policy_size,
        })
    }

    /// Returns the stable identity captured at admission.
    #[must_use]
    pub const fn fingerprint(&self) -> ValueFingerprint {
        self.fingerprint
    }

    /// Returns the retained policy size captured at admission.
    #[must_use]
    pub const fn policy_size(&self) -> usize {
        self.policy_size
    }

    /// Applies the admitted policy.
    pub fn transfer<S>(&self, state: &S) -> S
    where
        P: TransferPolicy<S>,
    {
        self.policy.transfer(state)
    }

    /// Returns the admitted policy object.
    #[must_use]
    pub const fn policy(&self) -> &P {
        &self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl StateSize for u8 {
        fn state_size(&self) -> usize {
            size_of::<Self>()
        }
    }

    impl JoinSemilattice for u8 {
        fn bottom(&self) -> Self {
            0
        }

        fn join(&self, other: &Self) -> Self {
            *self | *other
        }

        fn less_equal(&self, other: &Self) -> bool {
            self & other == *self
        }
    }

    #[derive(Clone, Debug)]
    struct AddFacts(u8);

    impl TransferPolicy<u8> for AddFacts {
        fn fingerprint(&self) -> ValueFingerprint {
            ValueFingerprint::new(u64::from(self.0))
        }

        fn policy_size(&self) -> usize {
            size_of::<Self>()
        }

        fn transfer(&self, state: &u8) -> u8 {
            state | self.0
        }
    }

    #[test]
    fn public_law_suite_admits_sound_policy_and_accounts_identity() {
        let samples = [0, 1, 2, 3];
        LawSuite::check_lattice(&samples).unwrap();
        LawSuite::check_transfer(&AddFacts(2), &samples).unwrap();

        let admitted = AdmittedTransfer::admit(AddFacts(2), &samples).unwrap();
        assert_eq!(admitted.fingerprint(), ValueFingerprint::new(2));
        assert_eq!(admitted.policy_size(), 1);
        assert_eq!(admitted.transfer(&1), 3);
        assert_eq!(3_u8.state_size(), 1);
    }

    #[derive(Clone, Debug)]
    struct NonMonotone;

    impl TransferPolicy<u8> for NonMonotone {
        fn fingerprint(&self) -> ValueFingerprint {
            ValueFingerprint::new(99)
        }

        fn policy_size(&self) -> usize {
            0
        }

        fn transfer(&self, state: &u8) -> u8 {
            if *state == 0 { 3 } else { *state }
        }
    }

    #[test]
    fn non_monotone_transfer_is_refused_at_admission_with_named_law() {
        let refusal = AdmittedTransfer::admit(NonMonotone, &[0, 1, 2, 3]).unwrap_err();
        assert_eq!(refusal.law(), DataflowLaw::TransferMonotone);
        assert_eq!(refusal.witnesses(), &[0, 1]);
    }
}
