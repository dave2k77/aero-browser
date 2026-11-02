//! Verification data are not authority: decoded reports can be forged.
//! Trusted host code owns readback adapters and must establish provenance,
//! freshness, accepted-contract binding and privacy before using a report.
use crate::actions::Action;
use crate::checkpoint::{MAX_COUNTER, RunContext, validate_references};
use crate::events::VerificationStatus;
use crate::form::FieldValue;
use crate::ids::{ArtifactId, DocumentEpoch, EvidenceId, TargetId};
use crate::task::{MAX_ITEMS, MAX_REFERENCE_BYTES, MAX_TEXT_BYTES};
use crate::wire::{ValidationError, require, strict_object, strict_tagged_object};

pub const VERIFICATION_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationState {
    Ready,
    Failed,
    Pending,
}

// This contract may contain private values. FieldState is only for nonsecret
// fields; trusted adapters must reject secret targets there, not return passwords.
strict_tagged_object! {
    #[derive(Eq)]
    pub enum Postcondition {
        Navigation { target: TargetId, document: DocumentEpoch, expected_url: String },
        FieldState { target: TargetId, locator: String, document: DocumentEpoch, expected: FieldValue },
        SecretIsSet { target: TargetId, locator: String, document: DocumentEpoch, expected: bool },
        ArtifactSchema { artifact: ArtifactId, columns: Vec<String> },
        ArtifactRowCount { artifact: ArtifactId, expected: u64 },
        EvidenceReference { artifact: ArtifactId, evidence: EvidenceId },
        Unsupported { checker: String },
    }
    tag "kind";
}

/// Internal, possibly private readback, never a public report. Its presence does
/// not prove source ownership or freshness: the trusted adapter must establish both.
#[derive(Clone, PartialEq, Eq)]
pub enum Observation {
    Navigation {
        target: TargetId,
        document: DocumentEpoch,
        effective_url: String,
        state: NavigationState,
    },
    FieldState {
        target: TargetId,
        locator: String,
        document: DocumentEpoch,
        actual: FieldValue,
    },
    SecretIsSet {
        target: TargetId,
        locator: String,
        document: DocumentEpoch,
        actual: bool,
    },
    ArtifactSchema {
        artifact: ArtifactId,
        columns: Vec<String>,
    },
    ArtifactRowCount {
        artifact: ArtifactId,
        actual: u64,
    },
    EvidenceReference {
        artifact: ArtifactId,
        evidence: EvidenceId,
        present: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckMethod {
    Navigation,
    FieldState,
    SecretIsSet,
    ArtifactSchema,
    ArtifactRowCount,
    EvidenceReference,
    Unsupported,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckReason {
    Matched,
    Mismatch,
    NavigationFailed,
    Pending,
    Unsupported,
    Unavailable,
    ScopeMismatch,
    InvalidReadback,
}

strict_object! {
    pub struct CriterionResult {
        pub schema_version: u32,
        pub context: RunContext,
        pub criterion_id: String,
        pub status: VerificationStatus,
        pub method: CheckMethod,
        pub evidence_ids: Vec<EvidenceId>,
        pub artifact_ids: Vec<ArtifactId>,
        pub checked_at_ms: u64,
        pub reason: CheckReason,
    }
    version schema_version = VERIFICATION_SCHEMA_VERSION;
}

fn reference(value: &str, field: &'static str) -> Result<(), ValidationError> {
    require(
        !value.trim().is_empty() && value.len() <= MAX_REFERENCE_BYTES && !value.contains('\0'),
        field,
        "invalid reference",
    )
}
fn navigation_url(value: &str) -> Result<(), ValidationError> {
    require(
        value.len() <= 2048,
        "verification.url",
        "URL exceeds 2048 bytes",
    )?;
    require(
        Action::Navigate {
            url: value.to_owned(),
        }
        .validate()
        .is_ok(),
        "verification.url",
        "unsupported URL shape",
    )
}
fn text(value: &str, field: &'static str) -> Result<(), ValidationError> {
    require(
        value.len() <= MAX_TEXT_BYTES && !value.contains('\0'),
        field,
        "invalid text",
    )
}
fn columns(values: &[String]) -> Result<(), ValidationError> {
    require(
        !values.is_empty() && values.len() <= MAX_ITEMS,
        "verification.columns",
        "invalid column count",
    )?;
    let mut seen = std::collections::HashSet::new();
    for value in values {
        reference(value, "verification.columns")?;
        require(
            seen.insert(value),
            "verification.columns",
            "duplicate column",
        )?;
    }
    Ok(())
}
fn field_value(value: &FieldValue) -> Result<(), ValidationError> {
    match value {
        FieldValue::Text { value } => text(value, "verification.field_value"),
        FieldValue::Checkbox { .. } => Ok(()),
        FieldValue::RadioGroup { selected_value } => {
            if let Some(value) = selected_value {
                text(value, "verification.field_value")
            } else {
                Ok(())
            }
        }
        FieldValue::Select { values } => {
            require(
                values.len() <= MAX_ITEMS,
                "verification.field_value",
                "too many values",
            )?;
            for value in values {
                text(value, "verification.field_value")?;
            }
            Ok(())
        }
    }
}
fn browser_identity(
    target: &TargetId,
    document: &DocumentEpoch,
    locator: Option<&str>,
) -> Result<(), ValidationError> {
    reference(target.as_str(), "verification.target")?;
    reference(document.as_str(), "verification.document")?;
    if let Some(locator) = locator {
        reference(locator, "verification.locator")?;
    }
    Ok(())
}
impl Postcondition {
    /// Structural limits only, not accepted intent, target sensitivity or freshness.
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Navigation {
                target,
                document,
                expected_url,
            } => {
                browser_identity(target, document, None)?;
                navigation_url(expected_url)
            }
            Self::FieldState {
                target,
                document,
                locator,
                expected,
            } => {
                browser_identity(target, document, Some(locator))?;
                field_value(expected)
            }
            Self::SecretIsSet {
                target,
                document,
                locator,
                ..
            } => browser_identity(target, document, Some(locator)),
            Self::ArtifactSchema {
                artifact,
                columns: c,
            } => {
                reference(artifact.as_str(), "verification.artifact")?;
                columns(c)
            }
            Self::ArtifactRowCount { artifact, expected } => {
                reference(artifact.as_str(), "verification.artifact")?;
                require(
                    *expected <= MAX_COUNTER,
                    "verification.count",
                    "count out of range",
                )
            }
            Self::EvidenceReference { artifact, evidence } => {
                reference(artifact.as_str(), "verification.artifact")?;
                reference(evidence.as_str(), "verification.evidence")
            }
            Self::Unsupported { checker } => reference(checker, "verification.checker"),
        }
    }
    pub fn method(&self) -> CheckMethod {
        match self {
            Self::Navigation { .. } => CheckMethod::Navigation,
            Self::FieldState { .. } => CheckMethod::FieldState,
            Self::SecretIsSet { .. } => CheckMethod::SecretIsSet,
            Self::ArtifactSchema { .. } => CheckMethod::ArtifactSchema,
            Self::ArtifactRowCount { .. } => CheckMethod::ArtifactRowCount,
            Self::EvidenceReference { .. } => CheckMethod::EvidenceReference,
            Self::Unsupported { .. } => CheckMethod::Unsupported,
        }
    }
}
impl Observation {
    pub fn validate(&self) -> Result<(), ValidationError> {
        match self {
            Self::Navigation {
                target,
                document,
                effective_url,
                state,
            } => {
                browser_identity(target, document, None)?;
                if *state == NavigationState::Ready {
                    navigation_url(effective_url)
                } else {
                    text(effective_url, "verification.url")
                }
            }
            Self::FieldState {
                target,
                document,
                locator,
                actual,
            } => {
                browser_identity(target, document, Some(locator))?;
                field_value(actual)
            }
            Self::SecretIsSet {
                target,
                document,
                locator,
                ..
            } => browser_identity(target, document, Some(locator)),
            Self::ArtifactSchema {
                artifact,
                columns: c,
            } => {
                reference(artifact.as_str(), "verification.artifact")?;
                columns(c)
            }
            Self::ArtifactRowCount { artifact, actual } => {
                reference(artifact.as_str(), "verification.artifact")?;
                require(
                    *actual <= MAX_COUNTER,
                    "verification.count",
                    "count out of range",
                )
            }
            Self::EvidenceReference {
                artifact, evidence, ..
            } => {
                reference(artifact.as_str(), "verification.artifact")?;
                reference(evidence.as_str(), "verification.evidence")
            }
        }
    }
}
/// Pure decision predicate. Callers must independently validate provenance,
/// criteria/artifact ownership and a current unresolved-effect snapshot.
pub fn completion_gate(
    required_checks: &[VerificationStatus],
    required_artifacts_verified: bool,
    unresolved_effect: bool,
) -> bool {
    !required_checks.is_empty()
        && required_checks
            .iter()
            .all(|s| *s == VerificationStatus::Passed)
        && required_artifacts_verified
        && !unresolved_effect
}

impl CriterionResult {
    /// Check structural consistency, never provenance, accepted contract or completion.
    pub fn validate_for(&self, expected: &RunContext) -> Result<(), ValidationError> {
        require(
            self.schema_version == VERIFICATION_SCHEMA_VERSION,
            "verification.schema_version",
            "unsupported version",
        )?;
        self.context.validate_for(expected)?;
        reference(self.context.task_id.as_str(), "verification.task_id")?;
        reference(self.context.run_id.as_str(), "verification.run_id")?;
        reference(&self.criterion_id, "verification.criterion_id")?;
        require(
            self.checked_at_ms > 0 && self.checked_at_ms <= MAX_COUNTER,
            "verification.checked_at_ms",
            "invalid timestamp",
        )?;
        require(
            self.evidence_ids.len() <= MAX_ITEMS && self.artifact_ids.len() <= MAX_ITEMS,
            "verification.references",
            "too many references",
        )?;
        validate_references(&self.evidence_ids, &self.artifact_ids)?;
        for id in &self.evidence_ids {
            reference(id.as_str(), "verification.evidence")?;
        }
        for id in &self.artifact_ids {
            reference(id.as_str(), "verification.artifact")?;
        }
        let valid = match self.reason {
            CheckReason::Matched => {
                self.status == VerificationStatus::Passed && self.method != CheckMethod::Unsupported
            }
            CheckReason::Mismatch | CheckReason::NavigationFailed => {
                self.status == VerificationStatus::Failed && self.method != CheckMethod::Unsupported
            }
            CheckReason::Pending
            | CheckReason::Unsupported
            | CheckReason::Unavailable
            | CheckReason::ScopeMismatch
            | CheckReason::InvalidReadback => self.status == VerificationStatus::Inconclusive,
        };
        require(
            valid,
            "verification.reason",
            "status and reason are inconsistent",
        )
    }
}

#[cfg(test)]
mod completion_tests {
    use super::*;

    #[test]
    fn completion_requires_checks_artifacts_and_resolved_effects() {
        assert!(!completion_gate(&[], true, false));
        for status in [
            VerificationStatus::NotChecked,
            VerificationStatus::Failed,
            VerificationStatus::Inconclusive,
        ] {
            assert!(!completion_gate(
                &[VerificationStatus::Passed, status],
                true,
                false
            ));
        }
        assert!(!completion_gate(
            &[VerificationStatus::Passed],
            false,
            false
        ));
        assert!(!completion_gate(&[VerificationStatus::Passed], true, true));
        assert!(completion_gate(&[VerificationStatus::Passed], true, false));
    }
}
