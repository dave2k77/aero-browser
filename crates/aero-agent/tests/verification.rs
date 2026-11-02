use aero_agent::verification::{
    Readback, ReadbackError, ReadbackQuery, ReadbackSource, VerifierRegistry,
};
use aero_core::checkpoint::RunContext;
use aero_core::events::{DispatchState, VerificationStatus};
use aero_core::form::FieldValue;
use aero_core::ids::{ArtifactId, DocumentEpoch, EvidenceId, RunId, TargetId, TaskId};
use aero_core::verification::{
    CheckMethod, CheckReason, CriterionResult, NavigationState, Observation, Postcondition,
};
use std::cell::{Cell, RefCell};

fn context() -> RunContext {
    RunContext {
        task_id: TaskId::from("synthetic-task"),
        run_id: RunId::from("synthetic-run"),
        contract_revision: 1,
    }
}

struct FakeReadback {
    stored: RefCell<FieldValue>,
}

// A synthetic dispatcher and independent readback share only the fake target
// state. Its acknowledgement does not promise that the requested value landed.
struct FakeDispatcher<'a> {
    target: &'a FakeReadback,
    applied: FieldValue,
}
impl FakeDispatcher<'_> {
    fn dispatch(&self, _requested: &FieldValue) -> DispatchState {
        *self.target.stored.borrow_mut() = self.applied.clone();
        DispatchState::Dispatched
    }
}

impl ReadbackSource for FakeReadback {
    fn read(
        &self,
        context: &RunContext,
        query: &ReadbackQuery<'_>,
    ) -> Result<Readback, ReadbackError> {
        assert!(
            matches!(query, ReadbackQuery::FieldState { target, document, locator } if target.as_str() == "target-1" && document.as_str() == "document-1" && *locator == "synthetic-field")
        );
        Ok(Readback {
            context: context.clone(),
            observation: Observation::FieldState {
                target: TargetId::from("target-1"),
                locator: "synthetic-field".into(),
                document: DocumentEpoch::from("document-1"),
                actual: self.stored.borrow().clone(),
            },
        })
    }
}

struct StaticSource {
    value: Result<Readback, ReadbackError>,
    calls: Cell<usize>,
}
impl ReadbackSource for StaticSource {
    fn read(&self, _: &RunContext, _: &ReadbackQuery<'_>) -> Result<Readback, ReadbackError> {
        self.calls.set(self.calls.get() + 1);
        self.value.clone()
    }
}
fn checked(
    condition: &Postcondition,
    observation: Observation,
    scope: RunContext,
) -> VerificationStatus {
    let source = StaticSource {
        value: Ok(Readback {
            context: scope,
            observation,
        }),
        calls: Cell::new(0),
    };
    VerifierRegistry::new(&source)
        .verify(&context(), "synthetic-check", condition, 1)
        .unwrap()
        .status
}

#[test]
fn navigation_and_artifact_checks_compare_independent_readbacks() {
    let target = TargetId::from("target");
    let doc = DocumentEpoch::from("doc");
    let artifact = ArtifactId::from("artifact");
    let evidence = EvidenceId::from("evidence");
    let nav = Postcondition::Navigation {
        target: target.clone(),
        document: doc.clone(),
        expected_url: "https://example.test/result".into(),
    };
    let navigation = |url: &str, state| Observation::Navigation {
        target: target.clone(),
        document: doc.clone(),
        effective_url: url.into(),
        state,
    };
    assert_eq!(
        checked(
            &nav,
            navigation("https://example.test/result", NavigationState::Ready),
            context()
        ),
        VerificationStatus::Passed
    );
    assert_eq!(
        checked(
            &nav,
            navigation("https://example.test/wrong", NavigationState::Ready),
            context()
        ),
        VerificationStatus::Failed
    );
    assert_eq!(
        checked(
            &nav,
            navigation("https://example.test/result", NavigationState::Pending),
            context()
        ),
        VerificationStatus::Inconclusive
    );
    assert_eq!(
        checked(
            &nav,
            navigation("https://example.test/result", NavigationState::Failed),
            context()
        ),
        VerificationStatus::Failed
    );
    let schema = Postcondition::ArtifactSchema {
        artifact: artifact.clone(),
        columns: vec!["supplier".into(), "price".into()],
    };
    let schema_actual = |cols: &[&str]| Observation::ArtifactSchema {
        artifact: artifact.clone(),
        columns: cols.iter().map(|s| (*s).into()).collect(),
    };
    assert_eq!(
        checked(&schema, schema_actual(&["supplier", "price"]), context()),
        VerificationStatus::Passed
    );
    assert_eq!(
        checked(&schema, schema_actual(&["price", "supplier"]), context()),
        VerificationStatus::Failed
    );
    let count = Postcondition::ArtifactRowCount {
        artifact: artifact.clone(),
        expected: 2,
    };
    assert_eq!(
        checked(
            &count,
            Observation::ArtifactRowCount {
                artifact: artifact.clone(),
                actual: 2
            },
            context()
        ),
        VerificationStatus::Passed
    );
    assert_eq!(
        checked(
            &count,
            Observation::ArtifactRowCount {
                artifact: artifact.clone(),
                actual: 1
            },
            context()
        ),
        VerificationStatus::Failed
    );
    let reference = Postcondition::EvidenceReference {
        artifact: artifact.clone(),
        evidence: evidence.clone(),
    };
    assert_eq!(
        checked(
            &reference,
            Observation::EvidenceReference {
                artifact: artifact.clone(),
                evidence: evidence.clone(),
                present: true
            },
            context()
        ),
        VerificationStatus::Passed
    );
    assert_eq!(
        checked(
            &reference,
            Observation::EvidenceReference {
                artifact,
                evidence,
                present: false
            },
            context()
        ),
        VerificationStatus::Failed
    );
}

#[test]
fn unknown_checker_and_wrong_scope_never_pass() {
    let source = StaticSource {
        value: Err(ReadbackError::Unsupported),
        calls: Cell::new(0),
    };
    let unsupported = Postcondition::Unsupported {
        checker: "future-checker".into(),
    };
    assert_eq!(
        VerifierRegistry::new(&source)
            .verify(&context(), "unknown", &unsupported, 1)
            .unwrap()
            .status,
        VerificationStatus::Inconclusive
    );
    assert_eq!(source.calls.get(), 0);
    let condition = Postcondition::ArtifactRowCount {
        artifact: ArtifactId::from("artifact"),
        expected: 1,
    };
    let same = || Observation::ArtifactRowCount {
        artifact: ArtifactId::from("artifact"),
        actual: 1,
    };
    let mut foreign = context();
    foreign.task_id = TaskId::from("other");
    assert_eq!(
        checked(&condition, same(), foreign),
        VerificationStatus::Inconclusive
    );
    assert_eq!(
        checked(
            &condition,
            Observation::ArtifactRowCount {
                artifact: ArtifactId::from("other"),
                actual: 1
            },
            context()
        ),
        VerificationStatus::Inconclusive
    );
}

#[test]
fn dispatch_ack_is_not_verification() {
    let requested = FieldValue::Text {
        value: "expected".into(),
    };
    let source = FakeReadback {
        stored: RefCell::new(FieldValue::Text {
            value: "before-dispatch".into(),
        }),
    };
    let dispatcher = FakeDispatcher {
        target: &source,
        applied: FieldValue::Text {
            value: "actual".into(),
        },
    };
    let condition = Postcondition::FieldState {
        target: TargetId::from("target-1"),
        locator: "synthetic-field".into(),
        document: DocumentEpoch::from("document-1"),
        expected: requested.clone(),
    };
    let dispatch = dispatcher.dispatch(&requested);
    let result = VerifierRegistry::new(&source)
        .verify(&context(), "field-check", &condition, 1)
        .unwrap();
    assert_eq!(dispatch, DispatchState::Dispatched);
    assert_eq!(result.status, VerificationStatus::Failed);
    let matching_dispatcher = FakeDispatcher {
        target: &source,
        applied: requested.clone(),
    };
    assert_eq!(
        matching_dispatcher.dispatch(&requested),
        DispatchState::Dispatched
    );
    assert_eq!(
        VerifierRegistry::new(&source)
            .verify(&context(), "field-check", &condition, 2)
            .unwrap()
            .status,
        VerificationStatus::Passed
    );
}

#[test]
fn failures_staleness_and_secrets_do_not_fabricate_proof() {
    let condition = Postcondition::SecretIsSet {
        target: TargetId::from("target"),
        document: DocumentEpoch::from("doc"),
        locator: "password-field".into(),
        expected: true,
    };
    let observation = |target: &str, document: &str, actual| Observation::SecretIsSet {
        target: TargetId::from(target),
        document: DocumentEpoch::from(document),
        locator: "password-field".into(),
        actual,
    };
    assert_eq!(
        checked(&condition, observation("target", "doc", true), context()),
        VerificationStatus::Passed
    );
    assert_eq!(
        checked(&condition, observation("target", "doc", false), context()),
        VerificationStatus::Failed
    );
    assert_eq!(
        checked(
            &condition,
            observation("target", "old-doc", true),
            context()
        ),
        VerificationStatus::Inconclusive
    );
    assert_eq!(
        checked(&condition, observation("other", "doc", true), context()),
        VerificationStatus::Inconclusive
    );
    for error in [ReadbackError::Unsupported, ReadbackError::Unavailable] {
        let source = StaticSource {
            value: Err(error),
            calls: Cell::new(0),
        };
        let result = VerifierRegistry::new(&source)
            .verify(&context(), "secret-check", &condition, 1)
            .unwrap();
        assert_eq!(result.status, VerificationStatus::Inconclusive);
        assert_eq!(
            result.reason,
            if error == ReadbackError::Unsupported {
                CheckReason::Unsupported
            } else {
                CheckReason::Unavailable
            }
        );
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("password-field")
        );
    }
    let malformed = Observation::ArtifactRowCount {
        artifact: ArtifactId::from("artifact"),
        actual: u64::MAX,
    };
    let count = Postcondition::ArtifactRowCount {
        artifact: ArtifactId::from("artifact"),
        expected: 2,
    };
    let source = StaticSource {
        value: Ok(Readback {
            context: context(),
            observation: malformed,
        }),
        calls: Cell::new(0),
    };
    let result = VerifierRegistry::new(&source)
        .verify(&context(), "count", &count, 1)
        .unwrap();
    assert_eq!(result.status, VerificationStatus::Inconclusive);
    assert_eq!(result.reason, CheckReason::InvalidReadback);
}

#[test]
fn invalid_requests_are_rejected_before_readback() {
    let source = StaticSource {
        value: Err(ReadbackError::Unavailable),
        calls: Cell::new(0),
    };
    let registry = VerifierRegistry::new(&source);
    let good = Postcondition::ArtifactRowCount {
        artifact: ArtifactId::from("artifact"),
        expected: 2,
    };
    let invalid = Postcondition::ArtifactRowCount {
        artifact: ArtifactId::from(""),
        expected: 2,
    };
    assert!(registry.verify(&context(), "check", &invalid, 1).is_err());
    assert!(registry.verify(&context(), "", &good, 1).is_err());
    assert!(registry.verify(&context(), "check", &good, 0).is_err());
    let invalid_url = Postcondition::Navigation {
        target: TargetId::from("target"),
        document: DocumentEpoch::from("doc"),
        expected_url: "not-a-url".into(),
    };
    assert!(
        registry
            .verify(&context(), "check", &invalid_url, 1)
            .is_err()
    );
    let mut invalid_context = context();
    invalid_context.contract_revision = 0;
    assert!(
        registry
            .verify(&invalid_context, "check", &good, 1)
            .is_err()
    );
    assert_eq!(source.calls.get(), 0);
}

#[test]
fn result_wire_is_strict_and_does_not_authorize_a_pass() {
    let source = StaticSource {
        value: Err(ReadbackError::Unavailable),
        calls: Cell::new(0),
    };
    let condition = Postcondition::ArtifactRowCount {
        artifact: ArtifactId::from("artifact"),
        expected: 2,
    };
    let result = VerifierRegistry::new(&source)
        .verify(&context(), "check", &condition, 1)
        .unwrap();
    let serialized = serde_json::to_string(&result).unwrap();
    let decoded: CriterionResult = serde_json::from_str(&serialized).unwrap();
    assert!(decoded == result);
    assert!(decoded.validate_for(&context()).is_ok());
    let mut invalid = decoded.clone();
    invalid.status = VerificationStatus::Passed;
    assert!(invalid.validate_for(&context()).is_err());
    // Shape validation is deliberately not provenance: a forged but consistent
    // pass decodes and validates. N05 must never promote this to authority.
    let mut forged = decoded.clone();
    forged.status = VerificationStatus::Passed;
    forged.reason = CheckReason::Matched;
    assert!(forged.validate_for(&context()).is_ok());
    let forged_json = serde_json::to_string(&forged).unwrap();
    let forged_roundtrip: CriterionResult = serde_json::from_str(&forged_json).unwrap();
    assert!(forged_roundtrip.validate_for(&context()).is_ok());
    invalid = decoded.clone();
    invalid.schema_version = 2;
    assert!(invalid.validate_for(&context()).is_err());
    let mut value: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    value["schema_version"] = 2.into();
    assert!(serde_json::from_value::<CriterionResult>(value).is_err());
    assert!(serde_json::from_str::<CriterionResult>("[]").is_err());
    let duplicate = serialized.replacen(
        "\"schema_version\":1",
        "\"schema_version\":1,\"schema_version\":1",
        1,
    );
    assert!(serde_json::from_str::<CriterionResult>(&duplicate).is_err());
    let invalid_postcondition =
        r#"{"kind":"artifact_row_count","artifact":"artifact","expected":2,"passed":true}"#;
    assert!(serde_json::from_str::<Postcondition>(invalid_postcondition).is_err());
    assert!(
        serde_json::from_str::<Postcondition>(
            r#"{"kind":"artifact_row_count","artifact":"artifact","expected":2}"#
        )
        .is_ok()
    );
    assert_eq!(result.method, CheckMethod::ArtifactRowCount);
}

#[test]
fn model_complete_cannot_bypass_criteria() {
    use aero_agent::store::worker::{Journal, Persistence, PrepareAction, Resources};
    use aero_agent::tasks::TaskAuthority;
    use aero_core::actions::{Action, Decision};
    use aero_core::ids::ObservationId;
    use aero_core::task::{StartPrerequisites, TaskContract};
    use std::os::unix::fs::PermissionsExt;

    let plan = include_str!("../../../docs/implementation-plan.md");
    let json = plan
        .split("### 7.10 TaskContract")
        .nth(1)
        .unwrap()
        .split("```json\n")
        .nth(1)
        .unwrap()
        .split("\n```")
        .next()
        .unwrap();
    let mut contract: TaskContract = serde_json::from_str(json).unwrap();
    contract.task_id = TaskId::from("synthetic-task");
    let parent = tempfile::tempdir().unwrap();
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = parent.path().join("journal");
    let journal = Journal::start(&path, 8).unwrap();
    let owner = TaskAuthority::new(&journal).trusted_user();
    owner
        .create_task(contract)
        .unwrap()
        .blocking_recv()
        .unwrap();
    owner
        .start_run(
            context(),
            StartPrerequisites {
                local_processor_available: true,
                background_service_available: false,
            },
        )
        .unwrap()
        .blocking_recv()
        .unwrap();
    // The host journal records a proposal; this is not an orchestrator and does
    // not invoke real completion checks or claim a completed transition.
    journal
        .client()
        .prepare_action(PrepareAction {
            context: context(),
            persistence: Persistence::Redacted,
            decision: Decision {
                schema_version: 1,
                observation_id: ObservationId::from("observation"),
                rationale: "synthetic".into(),
                action: Action::Complete {
                    summary: "model says done".into(),
                },
            },
            reserve: Resources {
                input_tokens: 0,
                output_tokens: 0,
                active_ms: 0,
            },
        })
        .unwrap()
        .blocking_recv()
        .unwrap();
    journal.close().unwrap();
    let db = rusqlite::Connection::open(path.join("journal.sqlite3")).unwrap();
    let state: (String, String, i64, i64) = db.query_row(
        "SELECT t.status, a.state, (SELECT count(*) FROM artifacts), (SELECT count(*) FROM criterion_results) FROM tasks t JOIN sessions s ON s.task_id=t.id JOIN actions a ON a.session_id=s.id WHERE t.id=?1",
        ["synthetic-task"], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap();
    assert_eq!(state, ("running".into(), "prepared".into(), 0, 0));
}

#[test]
fn planner_cannot_write_passed_criteria() {
    // Actual TaskClient API exclusion is enforced by the compile-fail example in
    // verification/mod.rs. The accepted task-contract wire also has no result setter.
    let injected = r#"{"kind":"row_count","id":"count","required":true,"artifact":"out","expected":2,"status":"passed"}"#;
    assert!(serde_json::from_str::<aero_core::task::Criterion>(injected).is_err());
    assert!(
        serde_json::from_str::<aero_core::task::Criterion>(
            r#"{"kind":"row_count","id":"count","required":true,"artifact":"out","expected":2}"#
        )
        .is_ok()
    );
}
