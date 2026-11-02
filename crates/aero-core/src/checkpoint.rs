//! Versioned pre-encryption checkpoint data. It is not a resume command or journal truth.
//!
//! Never persist this entire value into plaintext metadata_json: it contains form
//! values and URLs. The future store/encryption owner must split safe metadata
//! from authenticated encrypted snapshot bytes, with no plaintext fallback.
//! CaptureStatus is a producer report; empty forms do not prove complete capture.

use crate::form::FormSnapshot;
use crate::ids::{
    ArtifactId, DocumentEpoch, EvidenceId, FrameId, ObservationId, RunId, RuntimeEpoch, TargetId,
    TaskId,
};
use crate::wire::{ValidationError, nonempty, require, strict_object, unique_nonempty};

pub const CHECKPOINT_SCHEMA_VERSION: u32 = 1;
pub const MAX_COUNTER: u64 = i64::MAX as u64;
pub const MAX_SNAPSHOT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_CAPTURED_ORIGINS: usize = 20;
pub const MAX_SNAPSHOT_TABS: usize = 8;

strict_object! {
    pub struct RunContext {
        pub task_id: TaskId,
        pub run_id: RunId,
        pub contract_revision: u64,
    }
}
strict_object! {
    pub struct CumulativeUsage {
        pub attempts_spent: u64,
        pub tokens_spent: u64,
        pub tokens_reserved: u64,
        pub active_ms_spent: u64,
        pub recovery_count: u64,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    AwaitingUser,
    Paused,
    NeedsReview,
    Completed,
    Failed,
    Cancelled,
}

/// Capture producer's explicit report, not proof that capture actually occurred.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureStatus {
    Complete,
    Partial,
    Nonresumable,
}

strict_object! {
    pub struct DocumentSnapshot {
        pub frame_id: FrameId,
        pub document_epoch: DocumentEpoch,
        pub observation_id: ObservationId,
        pub origin: String,
        pub document_url: String,
        pub form: FormSnapshot,
    }
}
strict_object! {
    pub struct TabSnapshot {
        pub target_id: TargetId,
        pub documents: Vec<DocumentSnapshot>,
    }
}
strict_object! {
    pub struct Checkpoint {
        pub schema_version: u32,
        pub context: RunContext,
        pub step: u64,
        pub next_step: u64,
        pub usage: CumulativeUsage,
        pub status: RunStatus,
        pub capture_status: CaptureStatus,
        pub runtime_epoch: RuntimeEpoch,
        pub captured_origins: Vec<String>,
        pub tabs: Vec<TabSnapshot>,
        pub evidence_ids: Vec<EvidenceId>,
        pub artifact_ids: Vec<ArtifactId>,
    }
    version schema_version = CHECKPOINT_SCHEMA_VERSION;
}

impl RunContext {
    /// Validate reference shape only, not record existence or caller authority.
    pub fn validate(&self) -> Result<(), ValidationError> {
        nonempty(self.task_id.as_str(), "context.task_id")?;
        nonempty(self.run_id.as_str(), "context.run_id")?;
        bounded_counter(self.contract_revision, "context.contract_revision")?;
        require(
            self.contract_revision > 0,
            "context.contract_revision",
            "must be positive",
        )
    }

    /// `expected` must come from the trusted consumer, not the same input payload.
    pub fn validate_for(&self, expected: &Self) -> Result<(), ValidationError> {
        self.validate()?;
        expected.validate()?;
        require(
            self.task_id == expected.task_id,
            "context.task_id",
            "task mismatch",
        )?;
        require(
            self.run_id == expected.run_id,
            "context.run_id",
            "run mismatch",
        )?;
        require(
            self.contract_revision == expected.contract_revision,
            "context.contract_revision",
            "contract revision mismatch",
        )
    }
}

impl CumulativeUsage {
    /// Cumulative task counters, not per-run deltas, prices or a budget authority.
    pub fn validate(&self) -> Result<(), ValidationError> {
        for (value, field) in [
            (self.attempts_spent, "usage.attempts_spent"),
            (self.tokens_spent, "usage.tokens_spent"),
            (self.tokens_reserved, "usage.tokens_reserved"),
            (self.active_ms_spent, "usage.active_ms_spent"),
            (self.recovery_count, "usage.recovery_count"),
        ] {
            bounded_counter(value, field)?;
        }
        require(
            self.tokens_spent
                .checked_add(self.tokens_reserved)
                .is_some_and(|total| total <= MAX_COUNTER),
            "usage.tokens",
            "spent plus reserved exceeds the counter range",
        )
    }
}

pub(crate) fn bounded_counter(value: u64, field: &'static str) -> Result<(), ValidationError> {
    require(
        value <= MAX_COUNTER,
        field,
        "exceeds the SQLite integer range",
    )
}

pub(crate) fn validate_references(
    evidence: &[EvidenceId],
    artifacts: &[ArtifactId],
) -> Result<(), ValidationError> {
    unique_nonempty(evidence.iter().map(EvidenceId::as_str), "evidence_ids")?;
    unique_nonempty(artifacts.iter().map(ArtifactId::as_str), "artifact_ids")
}

impl Checkpoint {
    /// Structural consistency only. Resume must still consult the journal, task
    /// counters/grants, encrypted payload policy and fresh browser observations.
    pub fn validate_for(&self, expected: &RunContext) -> Result<(), ValidationError> {
        require(
            self.schema_version == CHECKPOINT_SCHEMA_VERSION,
            "checkpoint.schema_version",
            "unsupported checkpoint schema version",
        )?;
        self.context.validate_for(expected)?;
        self.usage.validate()?;
        bounded_counter(self.step, "checkpoint.step")?;
        bounded_counter(self.next_step, "checkpoint.next_step")?;
        require(
            self.next_step > self.step,
            "checkpoint.next_step",
            "must follow the committed step",
        )?;
        require(
            self.usage.attempts_spent >= self.next_step - 1,
            "usage.attempts_spent",
            "does not cover the reserved run cursor",
        )?;
        nonempty(self.runtime_epoch.as_str(), "checkpoint.runtime_epoch")?;
        require(
            self.captured_origins.len() <= MAX_CAPTURED_ORIGINS,
            "checkpoint.captured_origins",
            "must not exceed 20 origins",
        )?;
        require(
            self.tabs.len() <= MAX_SNAPSHOT_TABS,
            "checkpoint.tabs",
            "must not exceed 8 tabs",
        )?;
        unique_nonempty(
            self.captured_origins.iter().map(String::as_str),
            "checkpoint.captured_origins",
        )?;
        validate_references(&self.evidence_ids, &self.artifact_ids)?;
        let mut targets = std::collections::HashSet::new();
        let mut field_count = 0usize;
        for tab in &self.tabs {
            nonempty(tab.target_id.as_str(), "checkpoint.target_id")?;
            require(
                targets.insert(&tab.target_id),
                "checkpoint.tabs",
                "duplicate target",
            )?;
            let mut frames = std::collections::HashSet::new();
            for doc in &tab.documents {
                nonempty(doc.frame_id.as_str(), "checkpoint.frame_id")?;
                nonempty(doc.document_epoch.as_str(), "checkpoint.document_epoch")?;
                nonempty(doc.observation_id.as_str(), "checkpoint.observation_id")?;
                require(
                    frames.insert(&doc.frame_id),
                    "checkpoint.documents",
                    "duplicate frame in a tab",
                )?;
                require(
                    self.captured_origins.contains(&doc.origin),
                    "checkpoint.document.origin",
                    "origin is absent from captured inventory",
                )?;
                nonempty(&doc.document_url, "checkpoint.document_url")?;
                field_count = field_count.saturating_add(doc.form.fields.len());
                require(
                    field_count <= crate::form::MAX_FORM_ENTRIES,
                    "checkpoint.form_fields",
                    "must not exceed 2000 total entries",
                )?;
                doc.form.validate()?;
                for entry in &doc.form.fields {
                    require(
                        entry.locator.origin == doc.origin
                            && entry.locator.document_url == doc.document_url,
                        "checkpoint.form_scope",
                        "locator does not match its captured document",
                    )?;
                }
            }
        }
        Ok(())
    }
}
/// Check original encoded bytes before decoding. This is a size guard, not JSON
/// parsing, encryption, or proof that a subsequently supplied value is that data.
pub fn check_snapshot_size(encoded: &[u8]) -> Result<(), ValidationError> {
    require(
        encoded.len() <= MAX_SNAPSHOT_BYTES,
        "checkpoint.encoded_bytes",
        "must not exceed 2 MiB",
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::form::{FieldLocator, FieldValue, FormFieldEntry};
    use serde_json::json;

    pub(crate) fn context() -> RunContext {
        RunContext {
            task_id: TaskId::from("task-1"),
            run_id: RunId::from("run-1"),
            contract_revision: 1,
        }
    }

    pub(crate) fn usage() -> CumulativeUsage {
        CumulativeUsage {
            attempts_spent: 1,
            tokens_spent: 10,
            tokens_reserved: 5,
            active_ms_spent: 100,
            recovery_count: 0,
        }
    }

    pub(crate) fn fixture() -> Checkpoint {
        Checkpoint {
            schema_version: CHECKPOINT_SCHEMA_VERSION,
            context: context(),
            step: 1,
            next_step: 2,
            usage: usage(),
            status: RunStatus::Paused,
            capture_status: CaptureStatus::Complete,
            runtime_epoch: RuntimeEpoch::from("runtime-1"),
            captured_origins: vec!["https://merchant.test".into()],
            tabs: vec![TabSnapshot {
                target_id: TargetId::from("target-1"),
                documents: vec![DocumentSnapshot {
                    frame_id: FrameId::from("frame-1"),
                    document_epoch: DocumentEpoch::from("doc-1"),
                    observation_id: ObservationId::from("obs-1"),
                    origin: "https://merchant.test".into(),
                    document_url: "https://merchant.test/form".into(),
                    form: FormSnapshot {
                        fields: vec![FormFieldEntry {
                            locator: FieldLocator {
                                frame_path: vec![],
                                origin: "https://merchant.test".into(),
                                document_url: "https://merchant.test/form".into(),
                                shadow_hosts: vec![],
                                form_selector: None,
                                field_selector: "#name".into(),
                                tag: "input".into(),
                                input_type: "text".into(),
                                name: None,
                            },
                            value: FieldValue::Text {
                                value: "SYNTHETIC_PRIVATE_FORM_VALUE".into(),
                            },
                        }],
                    },
                }],
            }],
            evidence_ids: vec![EvidenceId::from("evidence-1")],
            artifact_ids: vec![ArtifactId::from("artifact-1")],
        }
    }

    #[test]
    fn checkpoint_capture_completeness_is_explicit() {
        for (status, wire) in [
            (CaptureStatus::Complete, "complete"),
            (CaptureStatus::Partial, "partial"),
            (CaptureStatus::Nonresumable, "nonresumable"),
        ] {
            let mut original = fixture();
            original.capture_status = status;
            let encoded = serde_json::to_value(&original).unwrap();
            assert_eq!(encoded["capture_status"], wire);
            assert!(serde_json::from_value::<Checkpoint>(encoded.clone()).unwrap() == original);
            assert_eq!(original.validate_for(&context()), Ok(()));
            let mut missing = encoded;
            missing.as_object_mut().unwrap().remove("capture_status");
            assert!(serde_json::from_value::<Checkpoint>(missing).is_err());
        }
    }

    #[test]
    fn checkpoint_version() {
        let original = fixture();
        let encoded = serde_json::to_string(&original).unwrap();
        assert!(serde_json::from_str::<Checkpoint>(&encoded).unwrap() == original);
        assert_eq!(original.validate_for(&context()), Ok(()));
        for version in [0, 2, u32::MAX] {
            let mut value = serde_json::to_value(&original).unwrap();
            value["schema_version"] = json!(version);
            assert!(serde_json::from_value::<Checkpoint>(value).is_err());
            let mut invalid = original.clone();
            invalid.schema_version = version;
            assert_eq!(
                invalid.validate_for(&context()).unwrap_err().field(),
                "checkpoint.schema_version"
            );
        }
        for version in [json!(null), json!("1"), json!(1.0), json!(-1)] {
            let mut value = serde_json::to_value(&original).unwrap();
            value["schema_version"] = version;
            assert!(serde_json::from_value::<Checkpoint>(value).is_err());
        }
        let mut value = serde_json::to_value(&original).unwrap();
        value.as_object_mut().unwrap().remove("schema_version");
        assert!(serde_json::from_value::<Checkpoint>(value).is_err());
    }

    #[test]
    fn checkpoint_rejects_wrong_context_and_empty_references() {
        for expected in [
            RunContext {
                task_id: TaskId::from("other"),
                ..context()
            },
            RunContext {
                run_id: RunId::from("other"),
                ..context()
            },
            RunContext {
                contract_revision: 2,
                ..context()
            },
            RunContext {
                contract_revision: 0,
                ..context()
            },
        ] {
            assert!(fixture().validate_for(&expected).is_err());
        }
        for mutate in [
            |c: &mut Checkpoint| c.context.task_id = TaskId::from(""),
            |c: &mut Checkpoint| c.context.run_id = RunId::from(""),
            |c: &mut Checkpoint| c.runtime_epoch = RuntimeEpoch::from(""),
            |c: &mut Checkpoint| c.tabs[0].target_id = TargetId::from(""),
            |c: &mut Checkpoint| c.tabs[0].documents[0].frame_id = FrameId::from(""),
            |c: &mut Checkpoint| c.tabs[0].documents[0].document_epoch = DocumentEpoch::from(""),
            |c: &mut Checkpoint| c.tabs[0].documents[0].observation_id = ObservationId::from(""),
            |c: &mut Checkpoint| c.evidence_ids[0] = EvidenceId::from(""),
            |c: &mut Checkpoint| c.artifact_ids[0] = ArtifactId::from(""),
        ] {
            let mut value = fixture();
            mutate(&mut value);
            assert!(value.validate_for(&context()).is_err());
        }
        let mut value = fixture();
        value.context.contract_revision = 0;
        assert!(value.validate_for(&value.context).is_err());
    }

    #[test]
    fn checkpoint_counters_are_bounded_and_cursor_is_reserved_once() {
        for mutate in [
            |c: &mut Checkpoint| c.step = MAX_COUNTER + 1,
            |c: &mut Checkpoint| c.next_step = 0,
            |c: &mut Checkpoint| c.next_step = 1,
            |c: &mut Checkpoint| c.next_step = MAX_COUNTER + 1,
            |c: &mut Checkpoint| c.usage.attempts_spent = 0,
            |c: &mut Checkpoint| {
                c.next_step = 4;
                c.usage.attempts_spent = 2;
            },
            |c: &mut Checkpoint| c.usage.attempts_spent = MAX_COUNTER + 1,
            |c: &mut Checkpoint| c.usage.tokens_spent = MAX_COUNTER + 1,
            |c: &mut Checkpoint| c.usage.tokens_reserved = MAX_COUNTER + 1,
            |c: &mut Checkpoint| c.usage.active_ms_spent = MAX_COUNTER + 1,
            |c: &mut Checkpoint| c.usage.recovery_count = MAX_COUNTER + 1,
            |c: &mut Checkpoint| {
                c.usage.tokens_spent = MAX_COUNTER;
                c.usage.tokens_reserved = 1;
            },
            |c: &mut Checkpoint| c.context.contract_revision = MAX_COUNTER + 1,
        ] {
            let mut value = fixture();
            mutate(&mut value);
            assert!(value.validate_for(&value.context).is_err());
        }
        let mut value = fixture();
        value.step = 0;
        value.next_step = 1;
        value.usage.attempts_spent = 0;
        assert_eq!(value.validate_for(&context()), Ok(()));
        value.step = 1;
        value.next_step = 4;
        value.usage.attempts_spent = 3;
        assert_eq!(value.validate_for(&context()), Ok(()));
        value.usage.attempts_spent = 100;
        assert_eq!(value.validate_for(&context()), Ok(()));
        value.step = MAX_COUNTER - 1;
        value.next_step = MAX_COUNTER;
        value.usage.attempts_spent = MAX_COUNTER - 1;
        value.usage.tokens_spent = MAX_COUNTER;
        value.usage.tokens_reserved = 0;
        assert_eq!(value.validate_for(&context()), Ok(()));
        let mut raw = serde_json::to_value(fixture()).unwrap();
        raw["step"] = json!(-1);
        assert!(serde_json::from_value::<Checkpoint>(raw).is_err());
    }

    #[test]
    fn checkpoint_inventory_scope_and_aggregate_limits() {
        for mutate in [
            |c: &mut Checkpoint| c.captured_origins.clear(),
            |c: &mut Checkpoint| c.captured_origins.push(c.captured_origins[0].clone()),
            |c: &mut Checkpoint| c.tabs.push(c.tabs[0].clone()),
            |c: &mut Checkpoint| {
                let duplicate = c.tabs[0].documents[0].clone();
                c.tabs[0].documents.push(duplicate);
            },
            |c: &mut Checkpoint| c.tabs[0].documents[0].origin = "other".into(),
            |c: &mut Checkpoint| c.tabs[0].documents[0].document_url.clear(),
            |c: &mut Checkpoint| {
                c.tabs[0].documents[0].form.fields[0].locator.document_url = "other".into()
            },
            |c: &mut Checkpoint| {
                c.tabs[0].documents[0].form.fields[0].locator.origin = "other".into()
            },
            |c: &mut Checkpoint| c.evidence_ids.push(c.evidence_ids[0].clone()),
            |c: &mut Checkpoint| c.artifact_ids.push(c.artifact_ids[0].clone()),
        ] {
            let mut value = fixture();
            mutate(&mut value);
            assert!(value.validate_for(&context()).is_err());
        }
        let mut value = fixture();
        value
            .captured_origins
            .extend((1..MAX_CAPTURED_ORIGINS).map(|i| format!("https://origin-{i}.test")));
        let template = value.tabs[0].clone();
        value.tabs = (0..MAX_SNAPSHOT_TABS)
            .map(|i| TabSnapshot {
                target_id: TargetId::from(format!("target-{i}")),
                ..template.clone()
            })
            .collect();
        assert_eq!(value.validate_for(&context()), Ok(()));
        value.tabs.push(TabSnapshot {
            target_id: TargetId::from("too-many"),
            ..template.clone()
        });
        assert!(value.validate_for(&context()).is_err());
        value.tabs.pop();
        value.captured_origins.push("https://one-more.test".into());
        assert!(value.validate_for(&context()).is_err());
        value = fixture();
        let field = value.tabs[0].documents[0].form.fields[0].clone();
        value.tabs[0].documents[0].form.fields = (0..crate::form::MAX_FORM_ENTRIES)
            .map(|i| {
                let mut entry = field.clone();
                entry.locator.field_selector = format!("#field-{i}");
                entry
            })
            .collect();
        assert_eq!(value.validate_for(&context()), Ok(()));
        let mut second = template;
        second.target_id = TargetId::from("second-tab");
        value.tabs.push(second);
        assert!(value.validate_for(&context()).is_err());
    }

    #[test]
    fn checkpoint_objects_reject_extra_duplicate_missing_and_array_fields() {
        let original = serde_json::to_value(fixture()).unwrap();
        for pointer in ["", "/context", "/usage", "/tabs/0", "/tabs/0/documents/0"] {
            let mut value = original.clone();
            value
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("grant".into(), json!(true));
            assert!(serde_json::from_value::<Checkpoint>(value).is_err());
            let fields = original.pointer(pointer).unwrap().as_object().unwrap();
            for key in fields.keys() {
                let mut value = original.clone();
                value
                    .pointer_mut(pointer)
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove(key);
                assert!(serde_json::from_value::<Checkpoint>(value).is_err());
            }
        }
        let encoded = serde_json::to_string(&fixture()).unwrap();
        for extra in [
            "\"schema_version\":1",
            "\"context\":{\"task_id\":\"x\",\"run_id\":\"x\",\"contract_revision\":1}",
        ] {
            let duplicate = format!("{{{extra},{}", &encoded[1..]);
            assert!(serde_json::from_str::<Checkpoint>(&duplicate).is_err());
        }
        assert!(serde_json::from_str::<RunContext>(r#"["task-1","run-1",1]"#).is_err());
        assert!(serde_json::from_str::<CumulativeUsage>("[1,10,5,100,0]").is_err());
        assert!(serde_json::from_str::<Checkpoint>("[]").is_err());
        assert!(serde_json::from_str::<Checkpoint>(&format!("{encoded} {{}}")).is_err());
        let mut bad = original;
        bad["status"] = json!("model_reported_success");
        assert!(serde_json::from_value::<Checkpoint>(bad).is_err());
    }

    #[test]
    fn checkpoint_encoded_payload_limit_is_checked_before_decoding() {
        assert_eq!(check_snapshot_size(&vec![b' '; MAX_SNAPSHOT_BYTES]), Ok(()));
        let error = check_snapshot_size(&vec![b'x'; MAX_SNAPSHOT_BYTES + 1]).unwrap_err();
        assert_eq!(error.field(), "checkpoint.encoded_bytes");
        assert!(!error.to_string().contains("SYNTHETIC"));
        let bytes = serde_json::to_vec(&fixture()).unwrap();
        assert_eq!(check_snapshot_size(&bytes), Ok(()));
    }
}
