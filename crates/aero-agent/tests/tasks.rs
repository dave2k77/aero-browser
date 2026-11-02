use aero_agent::store::StoreError;
use aero_agent::store::worker::{
    CommitStep, Journal, Persistence, PrepareAction, Resources, StepOutcome,
};
use aero_agent::tasks::TaskAuthority;
use aero_core::actions::{Action, Decision};
use aero_core::checkpoint::RunContext;
use aero_core::events::{DispatchState, VerificationStatus};
use aero_core::ids::{ObservationId, RunId, TaskId};
use aero_core::task::{Question, StartPrerequisites, TaskContract};
use rusqlite::Connection;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn directory() -> (tempfile::TempDir, PathBuf) {
    let parent = tempfile::tempdir().unwrap();
    std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = parent.path().join("journal");
    (parent, path)
}
fn contract(id: &str) -> TaskContract {
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
    contract.task_id = TaskId::from(id);
    contract
}
fn context(task: &str, run: &str, revision: u64) -> RunContext {
    RunContext {
        task_id: TaskId::from(task),
        run_id: RunId::from(run),
        contract_revision: revision,
    }
}
fn prerequisites() -> StartPrerequisites {
    StartPrerequisites {
        local_processor_available: true,
        background_service_available: false,
    }
}
fn raw(path: &Path) -> Connection {
    Connection::open(path.join("journal.sqlite3")).unwrap()
}

#[test]
fn contract_revision_requires_trusted_owner() {
    let (_parent, path) = directory();
    let journal = Journal::start(&path, 8).unwrap();
    let authority = TaskAuthority::new(&journal);
    let owner = authority.trusted_user();
    owner
        .create_task(contract("task"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    owner
        .start_run(context("task", "run", 1), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    let model = authority.model_for_task(TaskId::from("task")).unwrap();
    let mut revised = contract("task");
    revised.revision = 2;
    revised.title = "trusted revision".into();
    assert!(matches!(
        model.revise_contract(revised.clone()),
        Err(StoreError::PermissionDenied)
    ));
    assert!(matches!(
        model.create_task(contract("other")),
        Err(StoreError::PermissionDenied)
    ));
    assert!(matches!(
        model.start_run(context("task", "model-run", 1), prerequisites()),
        Err(StoreError::PermissionDenied)
    ));
    journal.close().unwrap();
    // Synthetic pre-existing grants, never a model-accessible grant API.
    let connection = raw(&path);
    connection.execute("INSERT INTO capability_grants VALUES ('grant','task',1,'trusted-fixture','{}','later',0,0)",[]).unwrap();
    connection.execute("INSERT INTO approvals VALUES ('approval','run','task',1,'obs','digest','{}','https://example.test','later',0)",[]).unwrap();
    drop(connection);
    let journal = Journal::start(&path, 8).unwrap();
    let authority = TaskAuthority::new(&journal);
    let owner = authority.trusted_user();
    owner
        .revise_contract(revised.clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        owner.revise_contract(revised).unwrap().blocking_recv(),
        Err(StoreError::Conflict)
    );
    let history = owner
        .load_resume(context("task", "run", 1))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(history.contract.revision, 1);
    owner
        .start_run(context("task", "new-run", 2), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    journal.close().unwrap();
    let connection = raw(&path);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM task_contracts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
    assert_eq!(
        connection
            .query_row(
                "SELECT contract_revision FROM sessions WHERE id='run'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT revoked FROM capability_grants", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT consumed FROM approvals", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn new_run_preserves_task_identity_and_history() {
    let (_parent, path) = directory();
    let journal = Journal::start(&path, 8).unwrap();
    let authority = TaskAuthority::new(&journal);
    let owner = authority.trusted_user();
    owner
        .create_task(contract("task"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let first = context("task", "first", 1);
    owner
        .start_run(first.clone(), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    // Trusted executor/verifier storage fixture, not an operation exposed by TaskClient.
    let storage = journal.client();
    let prepared = storage
        .prepare_action(PrepareAction {
            context: first.clone(),
            persistence: Persistence::Redacted,
            decision: Decision {
                schema_version: 1,
                observation_id: ObservationId::from("obs"),
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
        .unwrap();
    storage
        .commit_step(CommitStep {
            prepared,
            outcome: StepOutcome {
                schema_version: 1,
                dispatch: DispatchState::NotDispatched,
                verification: VerificationStatus::Passed,
            },
            actual: Some(Resources {
                input_tokens: 7,
                output_tokens: 3,
                active_ms: 80,
            }),
            evidence: vec![],
            artifacts: vec![],
        })
        .unwrap()
        .blocking_recv()
        .unwrap();
    let second = context("task", "second", 1);
    owner
        .start_run(second.clone(), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    let resumed = owner
        .load_resume(second.clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(resumed.context.task_id, first.task_id);
    assert_eq!(resumed.usage.attempts_spent, 1);
    assert_eq!(resumed.usage.tokens_spent, 10);
    assert_eq!(resumed.next_step, 1);
    let historical = owner
        .load_resume(first.clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        historical.current_status,
        aero_core::checkpoint::RunStatus::Cancelled
    );
    assert_eq!(
        historical.checkpoint.unwrap().status,
        aero_core::checkpoint::RunStatus::Paused
    );
    assert_eq!(
        owner
            .load_audit(first.clone(), 0, 10)
            .unwrap()
            .blocking_recv()
            .unwrap()
            .entries
            .len(),
        1
    );
    let reconnect = TaskAuthority::new(&journal)
        .model_for_task(TaskId::from("task"))
        .unwrap();
    assert_eq!(
        reconnect
            .load_resume(second)
            .unwrap()
            .blocking_recv()
            .unwrap()
            .usage
            .tokens_spent,
        10
    );
    assert_eq!(
        owner
            .start_run(first, prerequisites())
            .unwrap()
            .blocking_recv(),
        Err(StoreError::Database)
    );
    journal.close().unwrap();
    let connection = raw(&path);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM tasks", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM sessions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn start_checks_persisted_contract_prerequisites_without_writes() {
    let (_parent, path) = directory();
    let journal = Journal::start(&path, 8).unwrap();
    let owner = TaskAuthority::new(&journal).trusted_user();
    let mut c = contract("task");
    c.unresolved_questions.push(Question {
        id: "question".into(),
        prompt: "synthetic question".into(),
    });
    owner.create_task(c).unwrap().blocking_recv().unwrap();
    assert_eq!(
        owner
            .start_run(context("task", "blocked", 1), prerequisites())
            .unwrap()
            .blocking_recv(),
        Err(StoreError::StartBlocked)
    );
    let mut revised = contract("task");
    revised.revision = 2;
    owner
        .revise_contract(revised)
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        owner
            .start_run(
                context("task", "no-local", 2),
                StartPrerequisites {
                    local_processor_available: false,
                    background_service_available: false
                }
            )
            .unwrap()
            .blocking_recv(),
        Err(StoreError::StartBlocked)
    );
    owner
        .start_run(context("task", "allowed", 2), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    journal.close().unwrap();
    assert_eq!(
        raw(&path)
            .query_row("SELECT count(*) FROM sessions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn intent_and_run_records_do_not_claim_execution_readiness() {
    let (_parent, path) = directory();
    let journal = Journal::start(&path, 8).unwrap();
    let owner = TaskAuthority::new(&journal).trusted_user();
    owner
        .create_task(contract("task"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    journal.close().unwrap();
    assert_eq!(
        raw(&path)
            .query_row("SELECT status FROM tasks", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "draft"
    );
    let journal = Journal::start(&path, 8).unwrap();
    let owner = TaskAuthority::new(&journal).trusted_user();
    owner
        .start_run(context("task", "run", 1), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    journal.close().unwrap();
    let connection = raw(&path);
    assert_eq!(
        connection
            .query_row("SELECT status FROM tasks", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "paused"
    );
    assert_eq!(
        connection
            .query_row("SELECT status FROM sessions", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "paused"
    );
}

#[test]
fn concurrent_owner_revisions_are_serialized_and_history_stays_immutable() {
    let (_parent, path) = directory();
    let journal = Journal::start(&path, 8).unwrap();
    let owner = TaskAuthority::new(&journal).trusted_user();
    owner
        .create_task(contract("task"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let mut c = contract("task");
    c.revision = 2;
    let first = owner.revise_contract(c.clone()).unwrap();
    let second = owner.clone().revise_contract(c).unwrap();
    let results = [first.blocking_recv(), second.blocking_recv()];
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| **r == Err(StoreError::Conflict))
            .count(),
        1
    );
    assert_eq!(
        owner
            .start_run(context("task", "stale", 1), prerequisites())
            .unwrap()
            .blocking_recv(),
        Err(StoreError::Conflict)
    );
    owner
        .start_run(context("task", "current", 2), prerequisites())
        .unwrap()
        .blocking_recv()
        .unwrap();
    journal.close().unwrap();
    assert_eq!(
        raw(&path)
            .query_row("SELECT count(*) FROM task_contracts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn task_clients_remain_bound_to_their_journal_and_original_scope() {
    let (_parent, path) = directory();
    let (_other_parent, other_path) = directory();
    let a = Journal::start(&path, 8).unwrap();
    let b = Journal::start(&other_path, 8).unwrap();
    let aa = TaskAuthority::new(&a);
    let bb = TaskAuthority::new(&b);
    for (authority, title) in [(&aa, "first"), (&bb, "second")] {
        let owner = authority.trusted_user();
        let mut c = contract("same-id");
        c.title = title.into();
        owner.create_task(c).unwrap().blocking_recv().unwrap();
        owner
            .start_run(context("same-id", "same-run", 1), prerequisites())
            .unwrap()
            .blocking_recv()
            .unwrap();
    }
    for id in [
        "".to_owned(),
        " ".to_owned(),
        "nul\0id".to_owned(),
        "x".repeat(257),
    ] {
        assert!(matches!(
            aa.model_for_task(TaskId::from(id)),
            Err(StoreError::InvalidInput)
        ));
    }
    let model = aa.model_for_task(TaskId::from("same-id")).unwrap().clone();
    assert_eq!(
        model
            .load_resume(context("same-id", "same-run", 1))
            .unwrap()
            .blocking_recv()
            .unwrap()
            .contract
            .title,
        "first"
    );
    a.close().unwrap();
    assert!(matches!(
        model.load_resume(context("same-id", "same-run", 1)),
        Err(StoreError::WorkerClosed)
    ));
    assert_eq!(
        bb.model_for_task(TaskId::from("same-id"))
            .unwrap()
            .load_resume(context("same-id", "same-run", 1))
            .unwrap()
            .blocking_recv()
            .unwrap()
            .contract
            .title,
        "second"
    );
    b.close().unwrap();
}

#[test]
fn denied_model_calls_never_reach_a_closed_queue_or_payload_validator() {
    let (_parent, path) = directory();
    let journal = Journal::start(&path, 8).unwrap();
    let model = TaskAuthority::new(&journal)
        .model_for_task(TaskId::from("task"))
        .unwrap();
    journal.close().unwrap();
    let mut invalid = contract("task");
    invalid.schema_version = 999;
    invalid.title = "SYNTHETIC_PRIVATE".repeat(1000);
    assert!(matches!(
        model.create_task(invalid.clone()),
        Err(StoreError::PermissionDenied)
    ));
    assert!(matches!(
        model.revise_contract(invalid),
        Err(StoreError::PermissionDenied)
    ));
    assert!(matches!(
        model.start_run(context("", "", 0), prerequisites()),
        Err(StoreError::PermissionDenied)
    ));
    assert!(
        !StoreError::PermissionDenied
            .to_string()
            .contains("SYNTHETIC")
    );
    assert_eq!(
        raw(&path)
            .query_row("SELECT count(*) FROM tasks", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn blocked_or_lost_ack_start_preserves_prior_run_and_recovers_durably() {
    let (_parent, path) = directory();
    let journal = Journal::start(&path, 8).unwrap();
    let owner = TaskAuthority::new(&journal).trusted_user();
    let mut c = contract("task");
    c.close_behavior = aero_core::task::CloseBehavior::Background;
    owner.create_task(c).unwrap().blocking_recv().unwrap();
    let available = StartPrerequisites {
        local_processor_available: true,
        background_service_available: true,
    };
    owner
        .start_run(context("task", "first", 1), available)
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        owner
            .start_run(context("task", "blocked", 1), prerequisites())
            .unwrap()
            .blocking_recv(),
        Err(StoreError::StartBlocked)
    );
    assert_eq!(
        owner
            .load_resume(context("task", "first", 1))
            .unwrap()
            .blocking_recv()
            .unwrap()
            .current_status,
        aero_core::checkpoint::RunStatus::Paused
    );
    drop(
        owner
            .start_run(context("task", "accepted", 1), available)
            .unwrap(),
    );
    journal.close().unwrap();
    let journal = Journal::start(&path, 8).unwrap();
    assert!(matches!(
        owner.load_resume(context("task", "accepted", 1)),
        Err(StoreError::WorkerClosed)
    ));
    let owner = TaskAuthority::new(&journal).trusted_user();
    assert_eq!(
        owner
            .load_resume(context("task", "first", 1))
            .unwrap()
            .blocking_recv()
            .unwrap()
            .current_status,
        aero_core::checkpoint::RunStatus::Cancelled
    );
    assert_eq!(
        owner
            .load_resume(context("task", "accepted", 1))
            .unwrap()
            .blocking_recv()
            .unwrap()
            .current_status,
        aero_core::checkpoint::RunStatus::Paused
    );
    journal.close().unwrap();
    assert_eq!(
        raw(&path)
            .query_row("SELECT count(*) FROM sessions", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn model_scope_cannot_read_other_tasks_or_forge_run_context() {
    let (_parent, path) = directory();
    let journal = Journal::start(&path, 8).unwrap();
    let authority = TaskAuthority::new(&journal);
    let owner = authority.trusted_user();
    for task in ["a", "b"] {
        owner
            .create_task(contract(task))
            .unwrap()
            .blocking_recv()
            .unwrap();
        owner
            .start_run(context(task, task, 1), prerequisites())
            .unwrap()
            .blocking_recv()
            .unwrap();
    }
    let model = authority.model_for_task(TaskId::from("a")).unwrap();
    assert!(matches!(
        model.load_resume(context("b", "b", 1)),
        Err(StoreError::PermissionDenied)
    ));
    assert!(matches!(
        model.load_audit(context("b", "b", 1), 0, 10),
        Err(StoreError::PermissionDenied)
    ));
    assert!(matches!(
        model
            .load_resume(context("a", "b", 1))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::OwnershipMismatch)
    ));
    assert!(matches!(
        model
            .load_resume(context("a", "a", 2))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::OwnershipMismatch)
    ));
    assert_eq!(
        model
            .load_resume(context("a", "a", 1))
            .unwrap()
            .blocking_recv()
            .unwrap()
            .contract
            .task_id,
        TaskId::from("a")
    );
    journal.close().unwrap();
}
