use aero_agent::artifacts::ArtifactPersistence;
use aero_agent::config::StateConfig;
use aero_agent::evidence::{
    CapturedFact, EvidenceStore, FactCapture, StructuredClaim, assess_structured_claim,
};
use aero_agent::store::StoreError;
use aero_agent::store::worker::{
    CommitStep, EvidenceRecord, Journal, PrepareAction, PreparedAction, Resources, StepOutcome,
};
use aero_agent::tasks::TaskAuthority;
use aero_core::actions::{Action, Decision};
use aero_core::checkpoint::RunContext;
use aero_core::events::{DispatchState, VerificationStatus};
use aero_core::ids::{DocumentEpoch, EvidenceId, ObservationId, RunId, TaskId};
use aero_core::task::{StartPrerequisites, TaskContract};
use std::os::unix::fs::PermissionsExt;

fn context() -> RunContext {
    RunContext {
        task_id: TaskId::from("task"),
        run_id: RunId::from("run"),
        contract_revision: 1,
    }
}
fn fact() -> CapturedFact {
    CapturedFact {
        id: EvidenceId::from("capture"),
        context: context(),
        document: DocumentEpoch::from("document-1"),
        origin: "https://supplier.test".into(),
        effective_url: "https://supplier.test/catalog".into(),
        captured_at_ms: 1,
        entity: "supplier-a".into(),
        field: "price".into(),
        value: "42 USD".into(),
        excerpt: "price: 42 USD".into(),
    }
}
fn claim() -> StructuredClaim {
    StructuredClaim {
        evidence_id: EvidenceId::from("capture"),
        entity: "supplier-a".into(),
        field: "price".into(),
        value: "42 USD".into(),
    }
}

#[test]
fn absent_excerpt_and_contradictory_owned_facts_are_inconclusive() {
    let doc = DocumentEpoch::from("document-1");
    let mut missing_excerpt = fact();
    missing_excerpt.excerpt.clear();
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[missing_excerpt]),
        VerificationStatus::Inconclusive
    );
    let mut contradictory = fact();
    contradictory.id = EvidenceId::from("other-capture");
    contradictory.value = "43 USD".into();
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[fact(), contradictory]),
        VerificationStatus::Inconclusive
    );
    // Empty may be an actual source field value if an excerpt is present.
    let mut empty = fact();
    empty.value.clear();
    empty.excerpt = "price: [empty]".into();
    let mut empty_claim = claim();
    empty_claim.value.clear();
    assert_eq!(
        assess_structured_claim(&context(), &doc, &empty_claim, &[empty]),
        VerificationStatus::Passed
    );
}

#[test]
fn caller_can_forge_matching_capture_so_result_is_not_authority() {
    // These publicly constructible inputs are synthetic. A passing comparison
    // must never be interpreted as proof that a real source was captured.
    let forged = fact();
    assert_eq!(
        assess_structured_claim(
            &context(),
            &DocumentEpoch::from("document-1"),
            &claim(),
            &[forged]
        ),
        VerificationStatus::Passed
    );
}

#[test]
fn malformed_ambiguous_or_stale_inputs_cannot_pass() {
    let doc = DocumentEpoch::from("document-1");
    let valid = fact();
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[valid.clone(), valid.clone()]),
        VerificationStatus::Inconclusive
    );
    let mut bad = fact();
    bad.context.run_id = RunId::from("foreign");
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[bad]),
        VerificationStatus::Inconclusive
    );
    let mut bad = fact();
    bad.context.contract_revision = 2;
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[bad]),
        VerificationStatus::Inconclusive
    );
    let mut bad = fact();
    bad.captured_at_ms = 0;
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[bad]),
        VerificationStatus::Inconclusive
    );
    let mut bad = fact();
    bad.effective_url = "https://supplier.test.attacker/catalog".into();
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[bad]),
        VerificationStatus::Inconclusive
    );
    let mut bad = fact();
    bad.effective_url = "https://user@supplier.test/catalog".into();
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[bad]),
        VerificationStatus::Inconclusive
    );
    let mut bad = fact();
    bad.excerpt = "x".repeat(aero_core::task::MAX_TEXT_BYTES + 1);
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &[bad]),
        VerificationStatus::Inconclusive
    );
    let oversized = vec![valid; aero_core::task::MAX_ITEMS + 1];
    assert_eq!(
        assess_structured_claim(&context(), &doc, &claim(), &oversized),
        VerificationStatus::Inconclusive
    );
}

#[test]
fn claims_require_owned_captured_sources() {
    let current = DocumentEpoch::from("document-1");
    let capture = fact();
    assert_eq!(
        assess_structured_claim(
            &context(),
            &current,
            &claim(),
            std::slice::from_ref(&capture)
        ),
        VerificationStatus::Passed
    );
    let mut wrong = claim();
    wrong.value = "43 USD".into();
    assert_eq!(
        assess_structured_claim(&context(), &current, &wrong, std::slice::from_ref(&capture)),
        VerificationStatus::Failed
    );
    wrong = claim();
    wrong.field = "delivery".into();
    assert_eq!(
        assess_structured_claim(&context(), &current, &wrong, std::slice::from_ref(&capture)),
        VerificationStatus::Inconclusive
    );
    wrong = claim();
    wrong.entity = "supplier-b".into();
    assert_eq!(
        assess_structured_claim(&context(), &current, &wrong, std::slice::from_ref(&capture)),
        VerificationStatus::Inconclusive
    );
    wrong = claim();
    wrong.evidence_id = EvidenceId::from("unknown");
    assert_eq!(
        assess_structured_claim(&context(), &current, &wrong, std::slice::from_ref(&capture)),
        VerificationStatus::Inconclusive
    );
    let mut foreign = context();
    foreign.task_id = TaskId::from("other");
    assert_eq!(
        assess_structured_claim(&foreign, &current, &claim(), std::slice::from_ref(&capture)),
        VerificationStatus::Inconclusive
    );
    assert_eq!(
        assess_structured_claim(
            &context(),
            &DocumentEpoch::from("document-2"),
            &claim(),
            &[capture]
        ),
        VerificationStatus::Inconclusive
    );
}

fn capture() -> FactCapture {
    FactCapture {
        document: DocumentEpoch::from("document-1"),
        origin: "https://supplier.test".into(),
        effective_url: "https://supplier.test/catalog".into(),
        captured_at_ms: 1,
        entity: "supplier-a".into(),
        field: "price".into(),
        value: "42 USD".into(),
        excerpt: "price: 42 USD".into(),
    }
}
fn contract() -> TaskContract {
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
    contract.task_id = TaskId::from("task");
    contract
}
// Field order is drop order: stop the journal before removing its directory.
struct Fixture {
    journal: Journal,
    state: aero_agent::config::PrivateState,
    _home: tempfile::TempDir,
}
fn fixture() -> Fixture {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let state = StateConfig::resolve(Some(home.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let journal = Journal::start(state.journal_directory(), 8).unwrap();
    let owner = TaskAuthority::new(&journal).trusted_user();
    owner
        .create_task(contract())
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
    Fixture {
        _home: home,
        state,
        journal,
    }
}
fn prepare(journal: &Journal) -> PreparedAction {
    journal
        .client()
        .prepare_action(PrepareAction {
            context: context(),
            persistence: aero_agent::store::worker::Persistence::Redacted,
            decision: Decision {
                schema_version: 1,
                observation_id: ObservationId::from("observation"),
                rationale: "synthetic".into(),
                action: Action::Wait { milliseconds: 1 },
            },
            reserve: Resources {
                input_tokens: 10,
                output_tokens: 5,
                active_ms: 100,
            },
        })
        .unwrap()
        .blocking_recv()
        .unwrap()
}
fn commit(prepared: PreparedAction, evidence: Vec<EvidenceRecord>) -> CommitStep {
    CommitStep {
        prepared,
        outcome: StepOutcome {
            schema_version: 1,
            dispatch: DispatchState::NotDispatched,
            verification: VerificationStatus::Passed,
        },
        actual: Some(Resources {
            input_tokens: 1,
            output_tokens: 1,
            active_ms: 1,
        }),
        evidence,
        artifacts: vec![],
    }
}

#[test]
fn captured_facts_round_trip_through_the_journal_and_store() {
    let f = fixture();
    let lease = f.state.acquire_task(&TaskId::from("task")).unwrap();
    let store = EvidenceStore::open(&lease).unwrap();
    let directory = lease.directory().join("evidence");
    // Unclassified/sensitive and invalid captures write nothing.
    for policy in [ArtifactPersistence::Unknown, ArtifactPersistence::Sensitive] {
        assert_eq!(
            store.capture(policy, &context(), capture()).err(),
            Some(StoreError::EncryptionRequired)
        );
    }
    let mut invalid = capture();
    invalid.effective_url = "https://supplier.test.attacker/catalog".into();
    assert_eq!(
        store
            .capture(ArtifactPersistence::Public, &context(), invalid)
            .err(),
        Some(StoreError::InvalidInput)
    );
    assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);

    let staged = store
        .capture(ArtifactPersistence::Public, &context(), capture())
        .unwrap();
    let client = f.journal.client();
    client
        .commit_step(commit(prepare(&f.journal), vec![staged.record(context())]))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let record = client
        .load_evidence(TaskId::from("task"), staged.id().clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(record.id, *staged.id());
    assert_eq!(record.sha256, staged.sha256());
    assert_eq!(
        client
            .load_evidence(TaskId::from("task"), EvidenceId::from("absent"))
            .unwrap()
            .blocking_recv()
            .err(),
        Some(StoreError::NotFound)
    );
    // The fact used for assessment is the durable, read-back capture.
    let loaded: CapturedFact = store.load(&record).unwrap();
    assert_eq!(loaded.id, *staged.id());
    assert!(loaded.context == context());
    assert_eq!(loaded.value, "42 USD");
    let mut claim = claim();
    claim.evidence_id = staged.id().clone();
    assert_eq!(
        assess_structured_claim(
            &context(),
            &DocumentEpoch::from("document-1"),
            &claim,
            std::slice::from_ref(&loaded)
        ),
        VerificationStatus::Passed
    );
    // A record for another run or modified bytes cannot be loaded as a fact.
    let mut foreign = record.clone();
    foreign.context.run_id = RunId::from("other-run");
    assert_eq!(
        store.load(&foreign).err(),
        Some(StoreError::OwnershipMismatch)
    );
    let path = directory.join(staged.id().as_str());
    let tampered = std::fs::read_to_string(&path)
        .unwrap()
        .replace("42 USD", "43 USD");
    std::fs::write(&path, tampered).unwrap();
    assert_eq!(store.load(&record).err(), Some(StoreError::Conflict));
    f.journal.close().unwrap();
}

#[test]
fn journal_evidence_records_must_name_task_captures() {
    let f = fixture();
    let other = f.state.acquire_task(&TaskId::from("other")).unwrap();
    let mut foreign_context = context();
    foreign_context.task_id = TaskId::from("other");
    let foreign = EvidenceStore::open(&other)
        .unwrap()
        .capture(ArtifactPersistence::Public, &foreign_context, capture())
        .unwrap();
    let lease = f.state.acquire_task(&TaskId::from("task")).unwrap();
    let owned = EvidenceStore::open(&lease)
        .unwrap()
        .capture(ArtifactPersistence::Public, &context(), capture())
        .unwrap();
    let artifact_shaped = owned.id().as_str().replacen("e1-", "a1-", 1);
    let prepared = prepare(&f.journal);
    let client = f.journal.client();
    let fake = |id: &str| EvidenceRecord {
        id: EvidenceId::from(id),
        context: context(),
        sha256: "a".repeat(64),
        bytes: 1,
    };
    for (record, expected) in [
        (fake("evidence"), StoreError::InvalidInput),
        (fake(&artifact_shaped), StoreError::InvalidInput),
        (fake(foreign.id().as_str()), StoreError::OwnershipMismatch),
    ] {
        assert_eq!(
            client
                .commit_step(commit(prepared.clone(), vec![record]))
                .unwrap()
                .blocking_recv()
                .err(),
            Some(expected)
        );
    }
    client
        .commit_step(commit(prepared, vec![owned.record(context())]))
        .unwrap()
        .blocking_recv()
        .unwrap();
    f.journal.close().unwrap();

    let sql =
        rusqlite::Connection::open(f.state.journal_directory().join("journal.sqlite3")).unwrap();
    let id = owned.id().as_str();
    assert!(
        sql.execute(
            "UPDATE evidence SET metadata_json=json_set(metadata_json,'$.sha256',?2) WHERE id=?1",
            rusqlite::params![id, "0".repeat(64)]
        )
        .is_err()
    );
    assert!(
        sql.execute(
            "INSERT OR REPLACE INTO evidence SELECT * FROM evidence WHERE id=?1",
            [id]
        )
        .is_err()
    );
    assert!(
        sql.execute("DELETE FROM evidence WHERE id=?1", [id])
            .is_err()
    );
}
