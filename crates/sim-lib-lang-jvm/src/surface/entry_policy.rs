/// Caller-selected verification policy for one live JVM entry.
pub enum JvmEntryPolicy<'a> {
    /// Retain structural and runtime checks without claiming verifier fidelity.
    StaticChecked,
    /// Require one exact whole-class proof and converged frames for the selected method.
    Verified {
        /// Immutable whole-class proof produced by the verifier.
        proof: &'a ClassVerificationProof,
        /// Exact verifier policy/schema identity expected by the caller.
        policy: ValueFingerprint,
        /// Exact structural input identity expected by the caller.
        structural: ValueFingerprint,
        /// Converged entry frames for the selected method.
        frames: &'a [(sim_codec_classfile::InstructionId, VerificationState)],
    },
}
