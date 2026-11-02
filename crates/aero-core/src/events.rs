//! Public metadata reports, not planner commands, raw snapshots or proof of success.
//!
//! This is a structural boundary, not content sanitization: opaque IDs preserve
//! arbitrary strings. Trusted producers must issue/resolve references and enforce
//! provenance/privacy before publishing. Decoded reports cannot grant authority.

use crate::checkpoint::{
    CumulativeUsage, RunContext, RunStatus, bounded_counter, validate_references,
};
use crate::ids::{ArtifactId, EvidenceId};
use crate::wire::{ValidationError, require, strict_object, strict_tagged_object};

pub const EVENT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchState {
    /// Verification may check a pre-existing postcondition without dispatching an action.
    NotDispatched,
    Dispatched,
    Uncertain,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    NotChecked,
    Passed,
    Failed,
    Inconclusive,
}

strict_tagged_object! {
    #[derive(Eq)]
    pub enum EventKind {
        StatusChanged {
            status: RunStatus,
        },
        UsageUpdated {
            usage: CumulativeUsage,
        },
        StepOutcome {
            step: u64,
            dispatch_state: DispatchState,
            verification_status: VerificationStatus,
        },
    }
    tag "event";
}

strict_object! {
    pub struct PublicEvent {
        pub schema_version: u32,
        pub context: RunContext,
        pub sequence: u64,
        pub kind: EventKind,
        pub evidence_ids: Vec<EvidenceId>,
        pub artifact_ids: Vec<ArtifactId>,
    }
    version schema_version = EVENT_SCHEMA_VERSION;
}

impl PublicEvent {
    /// Validate report shape/context, not trusted provenance, event ordering,
    /// independent evidence or completion. The host service must establish those.
    pub fn validate_for(&self, expected: &RunContext) -> Result<(), ValidationError> {
        require(
            self.schema_version == EVENT_SCHEMA_VERSION,
            "event.schema_version",
            "unsupported event schema version",
        )?;
        self.context.validate_for(expected)?;
        bounded_counter(self.sequence, "event.sequence")?;
        require(self.sequence > 0, "event.sequence", "must be positive")?;
        validate_references(&self.evidence_ids, &self.artifact_ids)?;
        match &self.kind {
            EventKind::UsageUpdated { usage } => usage.validate()?,
            EventKind::StepOutcome {
                step,
                dispatch_state,
                verification_status,
            } => {
                bounded_counter(*step, "event.step")?;
                require(*step > 0, "event.step", "must be positive")?;
                require(
                    *dispatch_state != DispatchState::Uncertain
                        || *verification_status != VerificationStatus::Passed,
                    "event.verification_status",
                    "an uncertain effect cannot be reported as passed",
                )?;
            }
            // A status report does not perform the represented state transition.
            EventKind::StatusChanged { .. } => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkpoint::tests::{context, fixture, usage};
    use crate::ids::{ArtifactId, EvidenceId, RunId, TaskId};
    use serde_json::json;

    fn event(kind: EventKind) -> PublicEvent {
        PublicEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            context: context(),
            sequence: 1,
            kind,
            evidence_ids: vec![EvidenceId::from("evidence-1")],
            artifact_ids: vec![ArtifactId::from("artifact-1")],
        }
    }

    #[test]
    fn public_event_version_and_every_kind_round_trip() {
        let mut kinds = vec![EventKind::UsageUpdated { usage: usage() }];
        for status in [
            RunStatus::Running,
            RunStatus::AwaitingUser,
            RunStatus::Paused,
            RunStatus::NeedsReview,
            RunStatus::Completed,
            RunStatus::Failed,
            RunStatus::Cancelled,
        ] {
            kinds.push(EventKind::StatusChanged { status });
        }
        for verification in [
            VerificationStatus::NotChecked,
            VerificationStatus::Passed,
            VerificationStatus::Failed,
            VerificationStatus::Inconclusive,
        ] {
            for dispatch_state in [DispatchState::NotDispatched, DispatchState::Dispatched] {
                kinds.push(EventKind::StepOutcome {
                    step: 1,
                    dispatch_state,
                    verification_status: verification,
                });
            }
        }
        for kind in kinds {
            let original = event(kind);
            let encoded = serde_json::to_string(&original).unwrap();
            assert!(serde_json::from_str::<PublicEvent>(&encoded).unwrap() == original);
            assert_eq!(original.validate_for(&context()), Ok(()));
        }
        for version in [0, 2, u32::MAX] {
            let mut original = event(EventKind::StatusChanged {
                status: RunStatus::Running,
            });
            original.schema_version = version;
            assert!(original.validate_for(&context()).is_err());
            assert!(
                serde_json::from_str::<PublicEvent>(&serde_json::to_string(&original).unwrap())
                    .is_err()
            );
        }
    }

    #[test]
    fn public_event_rejects_context_counter_and_reference_errors() {
        let original = event(EventKind::UsageUpdated { usage: usage() });
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
        ] {
            assert!(original.validate_for(&expected).is_err());
        }
        for mutate in [
            |e: &mut PublicEvent| e.sequence = 0,
            |e: &mut PublicEvent| e.sequence = crate::checkpoint::MAX_COUNTER + 1,
            |e: &mut PublicEvent| e.context.task_id = TaskId::from(""),
            |e: &mut PublicEvent| e.context.contract_revision = 0,
            |e: &mut PublicEvent| e.evidence_ids.push(e.evidence_ids[0].clone()),
            |e: &mut PublicEvent| e.artifact_ids.push(e.artifact_ids[0].clone()),
            |e: &mut PublicEvent| e.evidence_ids[0] = EvidenceId::from(""),
            |e: &mut PublicEvent| e.artifact_ids[0] = ArtifactId::from(""),
            |e: &mut PublicEvent| {
                e.kind = EventKind::UsageUpdated {
                    usage: CumulativeUsage {
                        tokens_reserved: crate::checkpoint::MAX_COUNTER,
                        ..usage()
                    },
                }
            },
            |e: &mut PublicEvent| {
                e.kind = EventKind::StepOutcome {
                    step: 0,
                    dispatch_state: DispatchState::Dispatched,
                    verification_status: VerificationStatus::NotChecked,
                }
            },
            |e: &mut PublicEvent| {
                e.kind = EventKind::StepOutcome {
                    step: crate::checkpoint::MAX_COUNTER + 1,
                    dispatch_state: DispatchState::Dispatched,
                    verification_status: VerificationStatus::NotChecked,
                }
            },
        ] {
            let mut invalid = original.clone();
            mutate(&mut invalid);
            assert!(invalid.validate_for(&context()).is_err());
        }
        let mut max = original;
        max.sequence = crate::checkpoint::MAX_COUNTER;
        assert_eq!(max.validate_for(&context()), Ok(()));
    }

    #[test]
    fn public_event_dispatch_ack_is_not_verification() {
        let dispatched = event(EventKind::StepOutcome {
            step: 1,
            dispatch_state: DispatchState::Dispatched,
            verification_status: VerificationStatus::NotChecked,
        });
        let value = serde_json::to_value(&dispatched).unwrap();
        assert_eq!(value["kind"]["dispatch_state"], "dispatched");
        assert_eq!(value["kind"]["verification_status"], "not_checked");
        assert_eq!(dispatched.validate_for(&context()), Ok(()));
        let uncertain = event(EventKind::StepOutcome {
            step: 1,
            dispatch_state: DispatchState::Uncertain,
            verification_status: VerificationStatus::Passed,
        });
        assert!(uncertain.validate_for(&context()).is_err());
        for verification in [
            VerificationStatus::NotChecked,
            VerificationStatus::Failed,
            VerificationStatus::Inconclusive,
        ] {
            assert_eq!(
                event(EventKind::StepOutcome {
                    step: 1,
                    dispatch_state: DispatchState::Uncertain,
                    verification_status: verification
                })
                .validate_for(&context()),
                Ok(())
            );
        }
    }

    #[test]
    fn public_event_has_no_private_payload_or_authority_fields() {
        let original = event(EventKind::StatusChanged {
            status: RunStatus::NeedsReview,
        });
        let value = serde_json::to_value(&original).unwrap();
        let private = serde_json::to_value(fixture()).unwrap();
        for key in [
            "form",
            "tabs",
            "action",
            "rationale",
            "reasoning",
            "credentials",
            "grant",
            "role",
            "verified",
        ] {
            let mut bad = value.clone();
            bad[key] = private.clone();
            assert!(serde_json::from_value::<PublicEvent>(bad).is_err());
            let mut bad = value.clone();
            bad["kind"][key] = private.clone();
            assert!(serde_json::from_value::<PublicEvent>(bad).is_err());
        }
        assert!(
            !serde_json::to_string(&original)
                .unwrap()
                .contains("SYNTHETIC_PRIVATE_FORM_VALUE")
        );
        let mut bad = value;
        bad["kind"] = json!({"event":"complete","summary":"Trust me"});
        assert!(serde_json::from_value::<PublicEvent>(bad).is_err());
    }

    #[test]
    fn public_event_objects_are_strict_and_required() {
        let original = event(EventKind::StatusChanged {
            status: RunStatus::Running,
        });
        let value = serde_json::to_value(&original).unwrap();
        for key in value.as_object().unwrap().keys() {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(serde_json::from_value::<PublicEvent>(missing).is_err());
        }
        for raw in ["null", "[]", "[1,{},1,{},[],[]]"] {
            assert!(serde_json::from_str::<PublicEvent>(raw).is_err());
        }
        let encoded = serde_json::to_string(&original).unwrap();
        for extra in ["\"schema_version\":1", "\"sequence\":1"] {
            assert!(
                serde_json::from_str::<PublicEvent>(&format!("{{{extra},{}", &encoded[1..]))
                    .is_err()
            );
        }
        for raw in [
            r#"["status_changed","completed"]"#,
            r#"["usage_updated",{"attempts_spent":1,"tokens_spent":10,"tokens_reserved":5,"active_ms_spent":100,"recovery_count":0}]"#,
            r#"["step_outcome",1,"dispatched","passed"]"#,
        ] {
            assert!(
                serde_json::from_str::<EventKind>(raw).is_err(),
                "accepted EventKind array: {raw}"
            );
            let mut nested = value.clone();
            nested["kind"] = serde_json::from_str(raw).unwrap();
            assert!(serde_json::from_value::<PublicEvent>(nested).is_err());
        }
        for raw in [
            r#"{"event":"status_changed","status":"running","status":"completed"}"#,
            r#"{"event":"status_changed","event":"usage_updated","status":"running"}"#,
            r#"{"event":"status_changed"}"#,
        ] {
            assert!(serde_json::from_str::<EventKind>(raw).is_err());
        }
        let mut value = value;
        value["sequence"] = json!(-1);
        assert!(serde_json::from_value::<PublicEvent>(value).is_err());
        assert!(serde_json::from_str::<PublicEvent>(&format!("{encoded} {{}}")).is_err());
    }
}
