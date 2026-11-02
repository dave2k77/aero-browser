//! N16 coordinated task deletion with a permanent hashed tombstone (DEC-011).
//! Separate test binary: other suites spawn child processes that can briefly
//! inherit journal and task lock descriptors.

use aero_agent::artifacts::{ArtifactPersistence, ArtifactStore};
use aero_agent::config::{PrivateState, StateConfig};
use aero_agent::evidence::{EvidenceStore, FactCapture};
use aero_agent::store::StoreError;
use aero_agent::store::worker::{
    CommitStep, Journal, PrepareAction, PreparedAction, Resources, StepOutcome,
};
use aero_agent::tasks::TaskAuthority;
use aero_core::actions::{Action, Decision};
use aero_core::checkpoint::RunContext;
use aero_core::events::{DispatchState, VerificationStatus};
use aero_core::ids::{DocumentEpoch, ObservationId, RunId, RuntimeEpoch, TaskId};
use aero_core::task::{StartPrerequisites, TaskContract};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

fn contract(task: &str) -> TaskContract {
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
    contract.task_id = TaskId::from(task);
    contract
}
fn run(task: &str) -> RunContext {
    RunContext {
        task_id: TaskId::from(task),
        run_id: RunId::from(format!("{task}-run")),
        contract_revision: 1,
    }
}
fn prepare(journal: &Journal, context: &RunContext) -> PreparedAction {
    journal
        .client()
        .prepare_action(PrepareAction {
            context: context.clone(),
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

struct Fixture {
    journal: Journal,
    authority: TaskAuthority,
    state: PrivateState,
    home: tempfile::TempDir,
}
impl Fixture {
    fn new(tasks: &[&str]) -> Self {
        let home = tempfile::tempdir().unwrap();
        std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let state = StateConfig::resolve(Some(home.path()), None)
            .unwrap()
            .open()
            .unwrap();
        let journal = Journal::start(state.journal_directory(), 8).unwrap();
        let authority = TaskAuthority::new(&journal);
        let owner = authority.trusted_user();
        for task in tasks {
            owner
                .create_task(contract(task))
                .unwrap()
                .blocking_recv()
                .unwrap();
            owner
                .start_run(
                    run(task),
                    StartPrerequisites {
                        local_processor_available: true,
                        background_service_available: false,
                    },
                )
                .unwrap()
                .blocking_recv()
                .unwrap();
        }
        Self {
            journal,
            authority,
            state,
            home,
        }
    }
    /// Commit one step with a real artifact and evidence capture, and leave
    /// runtime directories with nested content plus a link pointing outside.
    fn populate(&self, task: &str) -> aero_agent::artifacts::StagedArtifact {
        let context = run(task);
        let lease = self.state.acquire_task(&TaskId::from(task)).unwrap();
        let artifact = ArtifactStore::open(&lease)
            .unwrap()
            .stage(ArtifactPersistence::Public, b"artifact")
            .unwrap();
        let evidence = EvidenceStore::open(&lease)
            .unwrap()
            .capture(
                ArtifactPersistence::Public,
                &context,
                FactCapture {
                    document: DocumentEpoch::from("document"),
                    origin: "https://supplier.test".into(),
                    effective_url: "https://supplier.test/x".into(),
                    captured_at_ms: 1,
                    entity: "e".into(),
                    field: "f".into(),
                    value: "v".into(),
                    excerpt: "f: v".into(),
                },
            )
            .unwrap();
        let runtime = lease
            .create_runtime(&context.run_id, &RuntimeEpoch::from("epoch"))
            .unwrap();
        std::fs::create_dir(runtime.profile_directory().join("Default")).unwrap();
        std::fs::write(runtime.profile_directory().join("Default/Cookies"), b"x").unwrap();
        symlink(self.outside(), runtime.staging_directory().join("escape")).unwrap();
        drop(runtime);
        self.journal
            .client()
            .commit_step(CommitStep {
                prepared: prepare(&self.journal, &context),
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
                evidence: vec![evidence.record(context.clone())],
                artifacts: vec![artifact.record(context.clone())],
            })
            .unwrap()
            .blocking_recv()
            .unwrap();
        artifact
    }
    fn outside(&self) -> std::path::PathBuf {
        let sentinel = self.home.path().join("outside-sentinel");
        if !sentinel.exists() {
            std::fs::write(&sentinel, b"keep").unwrap();
        }
        sentinel
    }
    fn task_dir(&self, task: &str) -> std::path::PathBuf {
        self.state
            .acquire_task(&TaskId::from(task))
            .unwrap()
            .directory()
            .to_path_buf()
    }
}
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn delete_task_purges_owned_derived_data() {
    let f = Fixture::new(&["owned", "other"]);
    f.populate("owned");
    let kept = f.populate("other");
    let owned_dir = f.task_dir("owned");
    let owner = f.authority.trusted_user();

    // A live store/lease for the task makes deletion Busy, with no change.
    let lease = f.state.acquire_task(&TaskId::from("owned")).unwrap();
    assert_eq!(
        f.authority
            .delete_task(&f.state, &TaskId::from("owned"))
            .err(),
        Some(StoreError::Busy)
    );
    assert!(owned_dir.join("artifacts").exists());
    owner
        .load_resume(run("owned"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    drop(lease);

    f.authority
        .delete_task(&f.state, &TaskId::from("owned"))
        .unwrap();
    // Only the permanent shell and its lock inode remain; links were removed,
    // never followed, and nothing outside the task was touched.
    assert_eq!(entries(&owned_dir), vec!["controller.lock".to_owned()]);
    assert_eq!(std::fs::read(f.outside()).unwrap(), b"keep");
    assert!(matches!(
        owner.load_resume(run("owned")).unwrap().blocking_recv(),
        Err(StoreError::NotFound)
    ));
    // The ID can never be reused in this state root.
    assert_eq!(
        owner
            .create_task(contract("owned"))
            .unwrap()
            .blocking_recv()
            .err(),
        Some(StoreError::Deleted)
    );
    assert_eq!(
        f.authority
            .delete_task(&f.state, &TaskId::from("owned"))
            .err(),
        Some(StoreError::Deleted)
    );
    // The other task's records and files are intact.
    let other_lease = f.state.acquire_task(&TaskId::from("other")).unwrap();
    let record = f
        .journal
        .client()
        .load_artifact(TaskId::from("other"), kept.id().clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        ArtifactStore::open(&other_lease)
            .unwrap()
            .verify(&record)
            .unwrap(),
        b"artifact"
    );
    drop(other_lease);
    f.journal.close().unwrap();

    // DEC-011: the tombstone holds only the hashed key, state and timestamps.
    let sql =
        rusqlite::Connection::open(f.state.journal_directory().join("journal.sqlite3")).unwrap();
    let key = owned_dir.file_name().unwrap().to_str().unwrap().to_owned();
    let (stored, state, columns): (String, String, i64) = sql
        .query_row(
            "SELECT task_key, state, (SELECT count(*) FROM pragma_table_info('task_tombstones')) FROM task_tombstones",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (stored.as_str(), state.as_str(), columns),
        (key.as_str(), "complete", 4)
    );
    for table in ["tasks", "sessions", "evidence", "artifacts"] {
        let owned: i64 = sql
            .query_row(
                &format!(
                    "SELECT count(*) FROM {table} WHERE {} = 'owned'",
                    if table == "tasks" { "id" } else { "task_id" }
                ),
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(owned, 0, "{table}");
    }
    // Tombstones are permanent and only move deleting -> complete.
    assert!(sql.execute("DELETE FROM task_tombstones", []).is_err());
    assert!(
        sql.execute("UPDATE task_tombstones SET state='deleting'", [])
            .is_err()
    );
    assert!(
        sql.execute("UPDATE task_tombstones SET task_key=?1", [&"0".repeat(64)])
            .is_err()
    );
    assert!(
        sql.execute(
            "INSERT OR REPLACE INTO task_tombstones VALUES (?1,'complete','now','now')",
            [&key]
        )
        .is_err()
    );
}

#[test]
fn holds_and_unresolved_work_block_deletion_before_it_starts() {
    let f = Fixture::new(&["task"]);
    f.populate("task");
    let owner = f.authority.trusted_user();
    let model = f.authority.model_for_task(TaskId::from("task")).unwrap();
    assert!(matches!(
        model.place_hold(TaskId::from("task"), "legal".into()),
        Err(StoreError::PermissionDenied)
    ));
    owner
        .place_hold(TaskId::from("task"), "legal".into())
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        f.authority
            .delete_task(&f.state, &TaskId::from("task"))
            .err(),
        Some(StoreError::Held)
    );
    assert!(f.task_dir("task").join("artifacts").exists());
    assert!(matches!(
        model.release_hold(TaskId::from("task"), "legal".into()),
        Err(StoreError::PermissionDenied)
    ));
    owner
        .release_hold(TaskId::from("task"), "legal".into())
        .unwrap()
        .blocking_recv()
        .unwrap();
    // A prepared (unresolved) action blocks deletion too.
    let prepared = prepare(&f.journal, &run("task"));
    assert_eq!(
        f.authority
            .delete_task(&f.state, &TaskId::from("task"))
            .err(),
        Some(StoreError::NeedsReview)
    );
    assert!(f.task_dir("task").join("artifacts").exists());
    f.journal
        .client()
        .commit_step(CommitStep {
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
            evidence: vec![],
            artifacts: vec![],
        })
        .unwrap()
        .blocking_recv()
        .unwrap();
    f.authority
        .delete_task(&f.state, &TaskId::from("task"))
        .unwrap();
    // Once deletion has started, holds can no longer be placed.
    assert_eq!(
        owner
            .place_hold(TaskId::from("task"), "late".into())
            .unwrap()
            .blocking_recv()
            .err(),
        Some(StoreError::Deleted)
    );
    f.journal.close().unwrap();
}

#[test]
fn interrupted_deletion_resumes_after_restart() {
    let f = Fixture::new(&["task"]);
    f.populate("task");
    let dir = f.task_dir("task");
    // Journal half only: as if the host crashed before removing files.
    f.journal
        .client()
        .delete_task(TaskId::from("task"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert!(dir.join("artifacts").exists());
    assert_eq!(
        f.authority
            .trusted_user()
            .create_task(contract("task"))
            .unwrap()
            .blocking_recv()
            .err(),
        Some(StoreError::Deleted)
    );
    let Fixture {
        journal,
        authority,
        state,
        home,
    } = f;
    drop(authority);
    journal.close().unwrap();

    let journal = Journal::start(state.journal_directory(), 8).unwrap();
    let authority = TaskAuthority::new(&journal);
    assert_eq!(authority.resume_deletions(&state).unwrap(), 1);
    assert_eq!(entries(&dir), vec!["controller.lock".to_owned()]);
    assert_eq!(
        std::fs::read(home.path().join("outside-sentinel")).unwrap(),
        b"keep"
    );
    // Idempotent: nothing left to resume.
    assert_eq!(authority.resume_deletions(&state).unwrap(), 0);
    journal.close().unwrap();
}
