//! Fixed comparisons over host-installed readback providers, not a sandbox.
//! Host code must not give a registry, source, or raw journal to model adapters.
//! `&self` alone cannot prevent an implementation from mutating external state.
use aero_core::ValidationError;
use aero_core::checkpoint::RunContext;
use aero_core::events::VerificationStatus;
use aero_core::ids::{ArtifactId, DocumentEpoch, EvidenceId, TargetId};
use aero_core::verification::{
    CheckReason, CriterionResult, NavigationState, Observation, Postcondition,
    VERIFICATION_SCHEMA_VERSION,
};

/// A host-verified source returns scoped observations, not model assertions.
/// It must resolve issued IDs, reject stale/wrong-owner data, refuse raw secret
/// field reads, and report errors when readback or ownership is not established.
#[derive(Clone)]
pub struct Readback {
    pub context: RunContext,
    pub observation: Observation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadbackError {
    Unsupported,
    Unavailable,
}
/// Readback requests carry identities, never expected values or a pass status.
pub enum ReadbackQuery<'a> {
    Navigation {
        target: &'a TargetId,
        document: &'a DocumentEpoch,
    },
    FieldState {
        target: &'a TargetId,
        document: &'a DocumentEpoch,
        locator: &'a str,
    },
    SecretIsSet {
        target: &'a TargetId,
        document: &'a DocumentEpoch,
        locator: &'a str,
    },
    ArtifactSchema {
        artifact: &'a ArtifactId,
    },
    ArtifactRowCount {
        artifact: &'a ArtifactId,
    },
    EvidenceReference {
        artifact: &'a ArtifactId,
        evidence: &'a EvidenceId,
    },
}
pub trait ReadbackSource {
    fn read(
        &self,
        context: &RunContext,
        query: &ReadbackQuery<'_>,
    ) -> Result<Readback, ReadbackError>;
}
fn compare(matches: bool) -> (VerificationStatus, CheckReason) {
    if matches {
        (VerificationStatus::Passed, CheckReason::Matched)
    } else {
        (VerificationStatus::Failed, CheckReason::Mismatch)
    }
}
pub struct VerifierRegistry<'a> {
    source: &'a dyn ReadbackSource,
}
impl<'a> VerifierRegistry<'a> {
    /// Trusted bootstrap only. The source and expected condition must not be
    /// supplied by the planner; this type is not an authentication mechanism.
    pub fn new(source: &'a dyn ReadbackSource) -> Self {
        Self { source }
    }
    /// `checked_at_ms` is trusted host Unix time in milliseconds. The registry
    /// does not persist results, decide task completion or establish provenance.
    pub fn verify(
        &self,
        context: &RunContext,
        criterion_id: &str,
        condition: &Postcondition,
        checked_at_ms: u64,
    ) -> Result<CriterionResult, ValidationError> {
        condition.validate()?;
        let mut result = CriterionResult {
            schema_version: VERIFICATION_SCHEMA_VERSION,
            context: context.clone(),
            criterion_id: criterion_id.into(),
            status: VerificationStatus::Inconclusive,
            method: condition.method(),
            evidence_ids: vec![],
            artifact_ids: vec![],
            checked_at_ms,
            reason: CheckReason::Unsupported,
        };
        // Validate all input before calling any adapter.
        result.validate_for(context)?;
        let query = match condition {
            Postcondition::Navigation {
                target, document, ..
            } => Some(ReadbackQuery::Navigation { target, document }),
            Postcondition::FieldState {
                target,
                document,
                locator,
                ..
            } => Some(ReadbackQuery::FieldState {
                target,
                document,
                locator,
            }),
            Postcondition::SecretIsSet {
                target,
                document,
                locator,
                ..
            } => Some(ReadbackQuery::SecretIsSet {
                target,
                document,
                locator,
            }),
            Postcondition::ArtifactSchema { artifact, .. } => {
                Some(ReadbackQuery::ArtifactSchema { artifact })
            }
            Postcondition::ArtifactRowCount { artifact, .. } => {
                Some(ReadbackQuery::ArtifactRowCount { artifact })
            }
            Postcondition::EvidenceReference { artifact, evidence } => {
                Some(ReadbackQuery::EvidenceReference { artifact, evidence })
            }
            Postcondition::Unsupported { .. } => None,
        };
        let readback = match query {
            Some(query) => self.source.read(context, &query),
            None => Err(ReadbackError::Unsupported),
        };
        let (status, reason) = match readback {
            Err(ReadbackError::Unsupported) => {
                (VerificationStatus::Inconclusive, CheckReason::Unsupported)
            }
            Err(ReadbackError::Unavailable) => {
                (VerificationStatus::Inconclusive, CheckReason::Unavailable)
            }
            Ok(readback) if readback.context.validate_for(context).is_err() => {
                (VerificationStatus::Inconclusive, CheckReason::ScopeMismatch)
            }
            Ok(readback) if readback.observation.validate().is_err() => (
                VerificationStatus::Inconclusive,
                CheckReason::InvalidReadback,
            ),
            Ok(readback) => match (condition, readback.observation) {
                (
                    Postcondition::Navigation {
                        target,
                        document,
                        expected_url,
                    },
                    Observation::Navigation {
                        target: got_target,
                        document: got_document,
                        effective_url,
                        state,
                    },
                ) if *target == got_target && *document == got_document => match state {
                    NavigationState::Pending => {
                        (VerificationStatus::Inconclusive, CheckReason::Pending)
                    }
                    NavigationState::Failed => {
                        (VerificationStatus::Failed, CheckReason::NavigationFailed)
                    }
                    NavigationState::Ready => compare(*expected_url == effective_url),
                },
                (
                    Postcondition::FieldState {
                        target,
                        locator,
                        document,
                        expected,
                    },
                    Observation::FieldState {
                        target: got_target,
                        locator: got_locator,
                        document: got_document,
                        actual,
                    },
                ) if *target == got_target
                    && *locator == got_locator
                    && *document == got_document =>
                {
                    compare(*expected == actual)
                }
                (
                    Postcondition::SecretIsSet {
                        target,
                        locator,
                        document,
                        expected,
                    },
                    Observation::SecretIsSet {
                        target: got_target,
                        locator: got_locator,
                        document: got_document,
                        actual,
                    },
                ) if *target == got_target
                    && *locator == got_locator
                    && *document == got_document =>
                {
                    compare(*expected == actual)
                }
                (
                    Postcondition::ArtifactSchema { artifact, columns },
                    Observation::ArtifactSchema {
                        artifact: got_artifact,
                        columns: actual,
                    },
                ) if *artifact == got_artifact => {
                    result.artifact_ids.push(artifact.clone());
                    compare(*columns == actual)
                }
                (
                    Postcondition::ArtifactRowCount { artifact, expected },
                    Observation::ArtifactRowCount {
                        artifact: got_artifact,
                        actual,
                    },
                ) if *artifact == got_artifact => {
                    result.artifact_ids.push(artifact.clone());
                    compare(*expected == actual)
                }
                (
                    Postcondition::EvidenceReference { artifact, evidence },
                    Observation::EvidenceReference {
                        artifact: got_artifact,
                        evidence: got_evidence,
                        present,
                    },
                ) if *artifact == got_artifact && *evidence == got_evidence => {
                    result.artifact_ids.push(artifact.clone());
                    if present {
                        result.evidence_ids.push(evidence.clone());
                    }
                    compare(present)
                }
                _ => (VerificationStatus::Inconclusive, CheckReason::ScopeMismatch),
            },
        };
        result.status = status;
        result.reason = reason;
        result.validate_for(context)?;
        Ok(result)
    }
}
