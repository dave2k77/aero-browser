//! Structured source facts: an ephemeral comparison plus a private, durable
//! capture store.
//!
//! Inputs may contain private content. Do not log them. A fact supplied by a
//! caller is never authority: the host must independently establish capture
//! authorization, source identity, sensitivity and freshness. Completion (N05)
//! must load facts itself through `JournalClient::load_evidence` and
//! [`EvidenceStore::load`], which read back the bytes committed with a step's
//! outcome; it must not accept facts handed to it. Opaque IDs and a matching
//! string establish none of those properties, and this module does not claim
//! real browser capture.
use crate::artifacts::{ArtifactPersistence, ArtifactStore, EVIDENCE_PREFIX};
use crate::config::TaskLease;
use crate::store::StoreError;
use crate::store::worker::EvidenceRecord;
use aero_core::actions::Action;
use aero_core::checkpoint::{MAX_COUNTER, RunContext};
use aero_core::events::VerificationStatus;
use aero_core::ids::{DocumentEpoch, EvidenceId};
use aero_core::task::{MAX_ITEMS, MAX_REFERENCE_BYTES, MAX_TEXT_BYTES};
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct CapturedFact {
    pub id: EvidenceId,
    pub context: RunContext,
    pub document: DocumentEpoch,
    pub origin: String,
    pub effective_url: String,
    pub captured_at_ms: u64,
    pub entity: String,
    pub field: String,
    pub value: String,
    pub excerpt: String,
}
pub struct StructuredClaim {
    pub evidence_id: EvidenceId,
    pub entity: String,
    pub field: String,
    pub value: String,
}
fn reference(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_REFERENCE_BYTES && !value.contains('\0')
}
fn text(value: &str) -> bool {
    value.len() <= MAX_TEXT_BYTES && !value.contains('\0')
}
fn url_is_scoped(origin: &str, url: &str) -> bool {
    if origin.len() > 2048 || url.len() > 2048 || !url.starts_with(origin) {
        return false;
    }
    let Some((scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "http" | "https") || rest.is_empty() || rest.contains(['/', '?', '#']) {
        return false;
    }
    let suffix = &url[origin.len()..];
    if !suffix.is_empty() && !suffix.starts_with(['/', '?', '#']) {
        return false;
    }
    Action::Navigate {
        url: origin.to_owned(),
    }
    .validate()
    .is_ok()
        && Action::Navigate {
            url: url.to_owned(),
        }
        .validate()
        .is_ok()
}
/// Shared bounds for a captured source fact; the excerpt must be nonempty.
#[allow(clippy::too_many_arguments)]
fn source_valid(
    document: &DocumentEpoch,
    origin: &str,
    effective_url: &str,
    captured_at_ms: u64,
    entity: &str,
    field: &str,
    value: &str,
    excerpt: &str,
) -> bool {
    reference(document.as_str())
        && captured_at_ms != 0
        && captured_at_ms <= MAX_COUNTER
        && url_is_scoped(origin, effective_url)
        && reference(entity)
        && reference(field)
        && text(value)
        && text(excerpt)
        && !excerpt.trim().is_empty()
}
/// Compare exact structured values from the *same* entity and field. A missing,
/// invalid, ambiguous, cross-scope or stale input is inconclusive. Even Passed
/// is only a comparison; the trusted caller must establish the provenance and
/// real freshness of both its observation and the claimed artifact value.
pub fn assess_structured_claim(
    context: &RunContext,
    current_document: &DocumentEpoch,
    claim: &StructuredClaim,
    captures: &[CapturedFact],
) -> VerificationStatus {
    if context.validate().is_err()
        || !reference(context.task_id.as_str())
        || !reference(context.run_id.as_str())
        || !reference(current_document.as_str())
        || !reference(claim.evidence_id.as_str())
        || !reference(&claim.entity)
        || !reference(&claim.field)
        || !text(&claim.value)
        || captures.len() > MAX_ITEMS
    {
        return VerificationStatus::Inconclusive;
    }
    let mut found = captures
        .iter()
        .filter(|capture| capture.id == claim.evidence_id);
    let Some(capture) = found.next() else {
        return VerificationStatus::Inconclusive;
    };
    if found.next().is_some()
        || capture.context.validate_for(context).is_err()
        || !reference(capture.id.as_str())
        || !source_valid(
            &capture.document,
            &capture.origin,
            &capture.effective_url,
            capture.captured_at_ms,
            &capture.entity,
            &capture.field,
            &capture.value,
            &capture.excerpt,
        )
        || capture.document != *current_document
        || capture.entity != claim.entity
        || capture.field != claim.field
    {
        return VerificationStatus::Inconclusive;
    }
    // Conflicting same-generation scoped facts prevent a decisive comparison.
    // This only checks the supplied slice; it cannot establish exhaustiveness.
    if captures.iter().any(|other| {
        other.context.validate_for(context).is_ok()
            && other.document == *current_document
            && other.entity == claim.entity
            && other.field == claim.field
            && other.value != capture.value
    }) {
        return VerificationStatus::Inconclusive;
    }
    if capture.value == claim.value {
        VerificationStatus::Passed
    } else {
        VerificationStatus::Failed
    }
}

/// A host observation to persist. The store assigns the evidence ID and binds
/// the run context; neither is caller-chosen.
pub struct FactCapture {
    pub document: DocumentEpoch,
    pub origin: String,
    pub effective_url: String,
    pub captured_at_ms: u64,
    pub entity: String,
    pub field: String,
    pub value: String,
    pub excerpt: String,
}

/// Canonical persisted form. Versioned and strict; never logged.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredFact {
    schema_version: u32,
    id: EvidenceId,
    context: RunContext,
    document: DocumentEpoch,
    origin: String,
    effective_url: String,
    captured_at_ms: u64,
    entity: String,
    field: String,
    value: String,
    excerpt: String,
}

/// Receipt for a written and read-back capture; not journal metadata.
#[derive(Clone, Debug)]
pub struct StagedEvidence {
    id: EvidenceId,
    length: usize,
    sha256: String,
}
impl StagedEvidence {
    pub fn id(&self) -> &EvidenceId {
        &self.id
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    /// Journal attachment for the step outcome that captured this fact.
    pub fn record(&self, context: RunContext) -> EvidenceRecord {
        EvidenceRecord {
            id: self.id.clone(),
            context,
            sha256: self.sha256.clone(),
            bytes: self.length as u64,
        }
    }
}

/// Private task-owned capture store (`evidence/`, `e1-` names), with the same
/// lease binding, exclusive creation and read-back checks as artifacts.
pub struct EvidenceStore {
    blobs: ArtifactStore,
}
impl EvidenceStore {
    pub fn open(lease: &TaskLease) -> Result<Self, StoreError> {
        Ok(Self {
            blobs: ArtifactStore::open_namespace(lease, "evidence", EVIDENCE_PREFIX)?,
        })
    }

    /// Validate and persist one fact for `context`. Unknown/Sensitive content
    /// is refused because encrypted staging does not exist yet; Public/Redacted
    /// are trusted host assertions, not classification.
    pub fn capture(
        &self,
        persistence: ArtifactPersistence,
        context: &RunContext,
        fact: FactCapture,
    ) -> Result<StagedEvidence, StoreError> {
        if matches!(
            persistence,
            ArtifactPersistence::Unknown | ArtifactPersistence::Sensitive
        ) {
            return Err(StoreError::EncryptionRequired);
        }
        if context.validate().is_err()
            || !source_valid(
                &fact.document,
                &fact.origin,
                &fact.effective_url,
                fact.captured_at_ms,
                &fact.entity,
                &fact.field,
                &fact.value,
                &fact.excerpt,
            )
        {
            return Err(StoreError::InvalidInput);
        }
        // The store generates the ID; the persisted fact embeds it.
        let staged = self.blobs.stage_with(persistence, |id| {
            serde_json::to_vec(&StoredFact {
                schema_version: 1,
                id: EvidenceId::from(id),
                context: context.clone(),
                document: fact.document.clone(),
                origin: fact.origin.clone(),
                effective_url: fact.effective_url.clone(),
                captured_at_ms: fact.captured_at_ms,
                entity: fact.entity.clone(),
                field: fact.field.clone(),
                value: fact.value.clone(),
                excerpt: fact.excerpt.clone(),
            })
            .map_err(|_| StoreError::InvalidInput)
        })?;
        Ok(StagedEvidence {
            id: EvidenceId::from(staged.id().as_str()),
            length: staged.len(),
            sha256: staged.sha256().to_owned(),
        })
    }

    /// Read back the capture a durable journal record describes and decode it.
    /// The bytes must match the record and encode the same ID and run context.
    pub fn load(&self, record: &EvidenceRecord) -> Result<CapturedFact, StoreError> {
        let length = usize::try_from(record.bytes).map_err(|_| StoreError::BudgetExceeded)?;
        let bytes = self
            .blobs
            .read_checked(record.id.as_str(), length, &record.sha256)?;
        if bytes.first() != Some(&b'{') {
            return Err(StoreError::CorruptDatabase);
        }
        let stored: StoredFact =
            serde_json::from_slice(&bytes).map_err(|_| StoreError::CorruptDatabase)?;
        if stored.schema_version != 1 || stored.id != record.id {
            return Err(StoreError::CorruptDatabase);
        }
        if stored.context != record.context {
            return Err(StoreError::OwnershipMismatch);
        }
        Ok(CapturedFact {
            id: stored.id,
            context: stored.context,
            document: stored.document,
            origin: stored.origin,
            effective_url: stored.effective_url,
            captured_at_ms: stored.captured_at_ms,
            entity: stored.entity,
            field: stored.field,
            value: stored.value,
            excerpt: stored.excerpt,
        })
    }
}
