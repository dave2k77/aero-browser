//! Ephemeral structured comparison, not an evidence repository or provenance.
//!
//! Inputs may contain private content. Do not log or persist them. The host must
//! independently establish capture authorization, ownership, source identity,
//! sensitivity, actual artifact values and document freshness. Opaque IDs and a
//! matching string establish none of those. In particular this pure comparison
//! must never by itself authorize task completion or claim real browser capture.
use aero_core::actions::Action;
use aero_core::checkpoint::{MAX_COUNTER, RunContext};
use aero_core::events::VerificationStatus;
use aero_core::ids::{DocumentEpoch, EvidenceId};
use aero_core::task::{MAX_ITEMS, MAX_REFERENCE_BYTES, MAX_TEXT_BYTES};

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
        || !reference(capture.document.as_str())
        || capture.document != *current_document
        || capture.captured_at_ms == 0
        || capture.captured_at_ms > MAX_COUNTER
        || !url_is_scoped(&capture.origin, &capture.effective_url)
        || !reference(&capture.entity)
        || !reference(&capture.field)
        || !text(&capture.value)
        || !text(&capture.excerpt)
        || capture.excerpt.trim().is_empty()
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
