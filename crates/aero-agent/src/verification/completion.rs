//! N05 completion gate: trusted evaluation of a run's bound outputs.
//!
//! The host names which committed artifact is which contract output; it never
//! supplies results. This module reads every bound artifact back against its
//! immutable journal record and evaluates the accepted contract's criteria
//! itself. The journal then rechecks run, revision, unresolved work, budget and
//! record identity in the same transaction that records results and completes.
//!
//! Supported machine checks: `ArtifactSchema` (exact ordered CSV header) and
//! `RowCount` (CSV data records) on CSV outputs. `EvidenceCoverage` and checks
//! on other formats are Inconclusive (Unsupported), so a contract requiring
//! them cannot complete (DEC-009: no attestation). Passing proves the bytes
//! have that shape, not that their content is true or well sourced.

use crate::artifacts::ArtifactStore;
use crate::store::StoreError;
use crate::store::worker::JournalClient;
use aero_core::checkpoint::{MAX_COUNTER, RunContext};
use aero_core::events::VerificationStatus;
use aero_core::ids::ArtifactId;
use aero_core::task::{Criterion, OutputFormat, TaskContract};
use aero_core::verification::{
    CheckMethod, CheckReason, CriterionResult, VERIFICATION_SCHEMA_VERSION,
};
use std::collections::{HashMap, HashSet};

/// The host's statement that `artifact` (committed in this run) is the
/// contract output named `output`. Not evidence that it satisfies anything.
pub struct OutputBinding {
    pub output: String,
    pub artifact: ArtifactId,
}

/// Results computed by the gate, never supplied by the caller.
pub struct CompletionReport {
    /// True only if the journal recorded the run and task as completed.
    pub completed: bool,
    pub results: Vec<CriterionResult>,
}

/// What phase 1 hands to the journal's completion transaction.
pub(crate) struct Evaluation {
    pub(crate) results: Vec<CriterionResult>,
    /// Bound artifacts whose bytes matched their record: (id, sha256).
    pub(crate) verified: Vec<(ArtifactId, String)>,
    /// Contract output names whose bound artifact verified.
    pub(crate) verified_outputs: Vec<String>,
}

pub(crate) fn evaluate(
    contract: &TaskContract,
    context: &RunContext,
    store: &ArtifactStore,
    journal: &JournalClient,
    bindings: Vec<OutputBinding>,
) -> Result<Evaluation, StoreError> {
    let formats: HashMap<&str, OutputFormat> = contract
        .outputs
        .iter()
        .map(|o| (o.name.as_str(), o.format))
        .collect();
    let mut seen_outputs = HashSet::new();
    let mut seen_artifacts = HashSet::new();
    for binding in &bindings {
        if !formats.contains_key(binding.output.as_str())
            || !seen_outputs.insert(binding.output.as_str())
            || !seen_artifacts.insert(binding.artifact.as_str())
        {
            return Err(StoreError::InvalidInput);
        }
    }
    // Output name -> (artifact, verified bytes or None if readback failed).
    let mut outputs: HashMap<String, (ArtifactId, Option<Vec<u8>>)> = HashMap::new();
    let mut verified = Vec::new();
    let mut verified_outputs = Vec::new();
    for binding in bindings {
        let record = journal
            .load_artifact(context.task_id.clone(), binding.artifact.clone())?
            .blocking_recv()?;
        if record.context != *context {
            return Err(StoreError::OwnershipMismatch);
        }
        let bytes = match store.verify(&record) {
            Ok(bytes) => {
                verified.push((record.id.clone(), record.sha256.clone()));
                verified_outputs.push(binding.output.clone());
                Some(bytes)
            }
            Err(
                StoreError::Conflict
                | StoreError::NotFound
                | StoreError::UnsafePath
                | StoreError::BudgetExceeded,
            ) => None,
            Err(other) => return Err(other),
        };
        outputs.insert(binding.output, (binding.artifact, bytes));
    }
    let checked_at_ms = now_ms();
    let results = contract
        .criteria
        .iter()
        .map(|criterion| {
            let bound = outputs.get(criterion.artifact());
            let (status, method, reason) = assess(
                criterion,
                formats.get(criterion.artifact()).copied(),
                bound.map(|(_, bytes)| bytes.as_deref()),
            );
            CriterionResult {
                schema_version: VERIFICATION_SCHEMA_VERSION,
                context: context.clone(),
                criterion_id: criterion.id().to_owned(),
                status,
                method,
                evidence_ids: vec![],
                artifact_ids: bound.map(|(id, _)| vec![id.clone()]).unwrap_or_default(),
                checked_at_ms,
                reason,
            }
        })
        .collect();
    Ok(Evaluation {
        results,
        verified,
        verified_outputs,
    })
}

/// `bound` is None when no artifact is bound, Some(None) when its readback
/// failed, and Some(Some(bytes)) for verified bytes.
fn assess(
    criterion: &Criterion,
    format: Option<OutputFormat>,
    bound: Option<Option<&[u8]>>,
) -> (VerificationStatus, CheckMethod, CheckReason) {
    use VerificationStatus::{Failed, Inconclusive, Passed};
    let method = match criterion {
        Criterion::ArtifactSchema { .. } => CheckMethod::ArtifactSchema,
        Criterion::RowCount { .. } => CheckMethod::ArtifactRowCount,
        Criterion::EvidenceCoverage { .. } => {
            return (
                Inconclusive,
                CheckMethod::Unsupported,
                CheckReason::Unsupported,
            );
        }
    };
    let bytes = match bound {
        None => return (Inconclusive, method, CheckReason::Unavailable),
        Some(None) => return (Inconclusive, method, CheckReason::InvalidReadback),
        Some(Some(bytes)) => bytes,
    };
    if format != Some(OutputFormat::Csv) {
        return (
            Inconclusive,
            CheckMethod::Unsupported,
            CheckReason::Unsupported,
        );
    }
    let Some(table) = parse_csv(bytes) else {
        return (Inconclusive, method, CheckReason::InvalidReadback);
    };
    let matched = match criterion {
        Criterion::ArtifactSchema { columns, .. } => table.header == *columns,
        Criterion::RowCount { expected, .. } => table.rows == *expected,
        Criterion::EvidenceCoverage { .. } => unreachable!("handled above"),
    };
    if matched {
        (Passed, method, CheckReason::Matched)
    } else {
        (Failed, method, CheckReason::Mismatch)
    }
}

fn now_ms() -> u64 {
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(1);
    u64::try_from(ms)
        .unwrap_or(MAX_COUNTER)
        .clamp(1, MAX_COUNTER)
}

struct Table {
    header: Vec<String>,
    rows: u64,
}

/// Strict RFC 4180 reading: UTF-8, comma separators, CRLF or LF record ends,
/// double-quoted fields with `""` escapes, an optional final line ending, and
/// every record with the header's field count. Anything else is None.
fn parse_csv(bytes: &[u8]) -> Option<Table> {
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text
        .strip_suffix("\r\n")
        .or_else(|| text.strip_suffix('\n'))
        .unwrap_or(text);
    if text.is_empty() {
        return None;
    }
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut record = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut field_started_quoted = false;
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => {
                    quoted = false;
                    // A closing quote must end the field.
                    if !matches!(chars.peek(), None | Some(',' | '\n' | '\r')) {
                        return None;
                    }
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !field_started_quoted => {
                quoted = true;
                field_started_quoted = true;
            }
            '"' => return None,
            ',' => {
                record.push(std::mem::take(&mut field));
                field_started_quoted = false;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' => return None,
            '\n' => {
                record.push(std::mem::take(&mut field));
                field_started_quoted = false;
                records.push(std::mem::take(&mut record));
            }
            _ => field.push(c),
        }
    }
    if quoted {
        return None;
    }
    record.push(field);
    records.push(record);
    let header = records.remove(0);
    if header.iter().any(String::is_empty) || records.iter().any(|r| r.len() != header.len()) {
        return None;
    }
    Some(Table {
        header,
        rows: records.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::parse_csv;

    #[test]
    fn csv_reading_is_strict() {
        let t = parse_csv(b"a,b\r\n1,\"x,\"\"y\"\"\nz\"\n2,3").unwrap();
        assert_eq!(t.header, vec!["a", "b"]);
        assert_eq!(t.rows, 2);
        assert_eq!(parse_csv(b"a,b\n").unwrap().rows, 0);
        for bad in [
            &b""[..],
            b"\n",
            b"a,b\n1\n",
            b"a,b\n1,2,3\n",
            b"a,\"b\n",
            b"a,b\"c\n",
            b"a,\"b\"c\n",
            b"a,,b\n",
            b"a\rb\n",
            b"a,b\n\xff,1\n",
        ] {
            assert!(parse_csv(bad).is_none(), "{bad:?}");
        }
    }
}
