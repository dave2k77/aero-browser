use aero_agent::store::worker::{
    ActionState, ArtifactRecord, CommitStep, EvidenceRecord, Journal as RawJournal,
    JournalClient as RawJournalClient, PrepareAction, PreparedAction, Reply, Resources,
    StepOutcome,
};
use aero_agent::store::{Store, StoreError};
use aero_core::actions::{Action, Decision};
use aero_core::checkpoint::RunContext;
use aero_core::events::{DispatchState, VerificationStatus};
use aero_core::ids::{ArtifactId, EvidenceId, ObservationId, RunId, TaskId};
use aero_core::task::TaskContract;
use rusqlite::{Connection, params};
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

// Explicit trusted-host fixture adapter for the pre-N02 storage suite. Task
// writes now go through the real TaskAuthority; all other operations use the
// original journal client. No SQL bypass, fabricated replies or grant API.
struct Journal(RawJournal);
impl Journal {
    fn start(path: &Path, capacity: usize) -> Result<Self, StoreError> {
        RawJournal::start(path, capacity).map(Self)
    }
    fn client(&self) -> JournalClient {
        JournalClient {
            storage: self.0.client(),
            owner: aero_agent::tasks::TaskAuthority::new(&self.0).trusted_user(),
        }
    }
    fn close(self) -> Result<(), StoreError> {
        self.0.close()
    }
}
struct JournalClient {
    storage: RawJournalClient,
    owner: aero_agent::tasks::TaskClient,
}
impl std::ops::Deref for JournalClient {
    type Target = RawJournalClient;
    fn deref(&self) -> &Self::Target {
        &self.storage
    }
}
impl JournalClient {
    fn create_task(&self, c: TaskContract) -> Result<Reply<()>, StoreError> {
        self.owner.create_task(c)
    }
    fn revise_contract(&self, c: TaskContract) -> Result<Reply<()>, StoreError> {
        self.owner.revise_contract(c)
    }
    fn create_run(&self, c: RunContext) -> Result<Reply<()>, StoreError> {
        self.owner.start_run(
            c,
            aero_core::task::StartPrerequisites {
                local_processor_available: true,
                background_service_available: false,
            },
        )
    }
}

#[test]
fn private_state_and_task_lock() {
    use aero_agent::config::StateConfig;
    let (parent, _) = directory();
    let state = StateConfig::resolve(Some(parent.path()), None)
        .unwrap()
        .open()
        .unwrap();
    assert_eq!(state.root(), parent.path().join(".local/state/aerobrowser"));
    assert_eq!(
        fs::metadata(state.root()).unwrap().permissions().mode() & 0o7777,
        0o700
    );
    assert_eq!(
        fs::metadata(state.journal_directory())
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o700
    );
    let task = state.acquire_task(&TaskId::from("task-a")).unwrap();
    assert_eq!(
        fs::metadata(task.directory().join("controller.lock"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    assert!(matches!(
        state.acquire_task(&TaskId::from("task-a")),
        Err(StoreError::Busy)
    ));
    let other = state.acquire_task(&TaskId::from("task-b")).unwrap();
    assert_ne!(task.directory(), other.directory());
    let runtime = task
        .create_runtime(
            &RunId::from("run-a"),
            &aero_core::ids::RuntimeEpoch::from("epoch-a"),
        )
        .unwrap();
    for path in [runtime.profile_directory(), runtime.staging_directory()] {
        assert!(path.starts_with(task.directory()));
        assert_eq!(
            fs::metadata(path).unwrap().permissions().mode() & 0o7777,
            0o700
        );
    }
    assert_ne!(runtime.profile_directory(), runtime.staging_directory());
    assert!(matches!(
        task.create_runtime(
            &RunId::from("run-a"),
            &aero_core::ids::RuntimeEpoch::from("epoch-a")
        ),
        Err(StoreError::Conflict)
    ));
    drop(task);
    assert!(matches!(
        state.acquire_task(&TaskId::from("task-a")),
        Err(StoreError::Busy)
    ));
    drop(runtime);
    assert!(state.acquire_task(&TaskId::from("task-a")).is_ok());
}

#[test]
fn xdg_resolution_is_explicit_and_never_uses_global_tmp() {
    use aero_agent::config::StateConfig;
    let (parent, _) = directory();
    let explicit = parent.path().join("xdg");
    assert_eq!(
        StateConfig::resolve(None, Some(&explicit)).unwrap().root(),
        explicit.join("aerobrowser")
    );
    assert_eq!(
        StateConfig::resolve(Some(parent.path()), Some(Path::new("")))
            .unwrap()
            .root(),
        parent.path().join(".local/state/aerobrowser")
    );
    assert!(matches!(
        StateConfig::resolve(None, None),
        Err(StoreError::InvalidInput)
    ));
    assert!(matches!(
        StateConfig::resolve(Some(parent.path()), Some(Path::new("relative"))),
        Err(StoreError::InvalidInput)
    ));
    assert!(matches!(
        StateConfig::resolve(Some(Path::new("relative")), None),
        Err(StoreError::InvalidInput)
    ));
    assert!(matches!(
        StateConfig::resolve(None, Some(&parent.path().join("../escape"))),
        Err(StoreError::InvalidInput)
    ));
    assert!(!explicit.exists()); // Pure configuration does not create directories.
}

#[test]
fn task_controller_lock_cross_process() {
    use aero_agent::config::StateConfig;
    const CHILD: &str = "AERO_B06_CHILD";
    if let Some(mode) = std::env::var_os(CHILD) {
        let state = StateConfig::from_env().unwrap().open().unwrap();
        let result = state.acquire_task(&TaskId::from("task"));
        if mode == "busy" {
            assert!(matches!(result, Err(StoreError::Busy)));
        } else {
            assert!(result.is_ok());
        }
        assert!(
            state
                .acquire_task(&TaskId::from("independent-child-task"))
                .is_ok()
        );
        return;
    }
    let (parent, _) = directory();
    let base = parent.path().join("xdg");
    let state = StateConfig::resolve(None, Some(&base))
        .unwrap()
        .open()
        .unwrap();
    let task = state.acquire_task(&TaskId::from("task")).unwrap();
    let runtime = task
        .create_runtime(
            &RunId::from("run"),
            &aero_core::ids::RuntimeEpoch::from("epoch"),
        )
        .unwrap();
    let check = |mode: &str| {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "task_controller_lock_cross_process"])
            .env(CHILD, mode)
            .env("XDG_STATE_HOME", &base)
            .env_remove("HOME")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    check("busy");
    drop(task);
    check("busy");
    drop(runtime);
    check("free");
    assert!(state.acquire_task(&TaskId::from("task")).is_ok());
}

#[test]
fn opaque_ids_cannot_escape_or_alias_private_layouts() {
    use aero_agent::config::StateConfig;
    use aero_core::ids::RuntimeEpoch;
    let (parent, _) = directory();
    let state = StateConfig::resolve(Some(parent.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let names = ["../outside", "a/b", "a_b", "A", "a", "a ", "é"];
    let mut task_paths = std::collections::HashSet::new();
    for name in names {
        let task = state.acquire_task(&TaskId::from(name)).unwrap();
        assert!(task.directory().starts_with(state.root().join("tasks")));
        assert!(task_paths.insert(task.directory().to_path_buf()));
        let first = task
            .create_runtime(&RunId::from("../run"), &RuntimeEpoch::from("../epoch"))
            .unwrap();
        let second = task
            .create_runtime(&RunId::from("../run"), &RuntimeEpoch::from("next"))
            .unwrap();
        let third = task
            .create_runtime(&RunId::from("other"), &RuntimeEpoch::from("../epoch"))
            .unwrap();
        assert_ne!(first.profile_directory(), second.profile_directory());
        assert_ne!(first.profile_directory(), third.profile_directory());
        assert!(first.profile_directory().starts_with(task.directory()));
        let retained = first.profile_directory().to_path_buf();
        drop(first);
        assert!(retained.is_dir());
    }
    assert!(!parent.path().join("outside").exists());
    assert!(state.acquire_task(&TaskId::from("é".repeat(128))).is_ok());
    let before = fs::read_dir(state.root().join("tasks")).unwrap().count();
    for id in [
        "".to_owned(),
        " ".to_owned(),
        "x".repeat(257),
        "nul\0value".to_owned(),
    ] {
        assert!(matches!(
            state.acquire_task(&TaskId::from(id)),
            Err(StoreError::InvalidInput)
        ));
    }
    assert_eq!(
        fs::read_dir(state.root().join("tasks")).unwrap().count(),
        before
    );
    let task = state.acquire_task(&TaskId::from("valid")).unwrap();
    assert!(matches!(
        task.create_runtime(&RunId::from(""), &RuntimeEpoch::from("epoch")),
        Err(StoreError::InvalidInput)
    ));
    assert!(matches!(
        task.create_runtime(&RunId::from("run"), &RuntimeEpoch::from("")),
        Err(StoreError::InvalidInput)
    ));
}

#[test]
fn concurrent_and_partial_runtime_epochs_are_never_reused() {
    use aero_agent::config::StateConfig;
    use aero_core::ids::RuntimeEpoch;
    use sha2::{Digest, Sha256};
    let (parent, _) = directory();
    let state = StateConfig::resolve(Some(parent.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let task = state.acquire_task(&TaskId::from("task")).unwrap();
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    task.create_runtime(&RunId::from("same"), &RuntimeEpoch::from("same"))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(StoreError::Conflict)))
            .count(),
        1
    );
    let runtime = results.into_iter().find_map(Result::ok).unwrap();
    let sentinel = runtime.profile_directory().join("synthetic-sentinel");
    fs::write(&sentinel, b"retained").unwrap();
    assert!(matches!(
        task.create_runtime(&RunId::from("same"), &RuntimeEpoch::from("same")),
        Err(StoreError::Conflict)
    ));
    assert_eq!(fs::read(sentinel).unwrap(), b"retained");
    let hash = format!(
        "{:x}",
        Sha256::digest(b"aerobrowser-state-v1\0runtime\0partial")
    );
    let partial = runtime
        .profile_directory()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join(hash);
    fs::create_dir(&partial).unwrap();
    fs::set_permissions(&partial, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(
        task.create_runtime(&RunId::from("same"), &RuntimeEpoch::from("partial")),
        Err(StoreError::Conflict)
    ));
    assert_eq!(fs::read_dir(partial).unwrap().count(), 0);
    let run_key = runtime
        .profile_directory()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .file_name()
        .unwrap();
    let epoch_key = runtime
        .profile_directory()
        .parent()
        .unwrap()
        .file_name()
        .unwrap();
    assert_ne!(run_key, epoch_key); // Same opaque bytes, different domain separators.
}

#[test]
fn private_state_supports_os_native_non_unicode_paths() {
    use aero_agent::config::StateConfig;
    use std::os::unix::ffi::OsStringExt;
    let (parent, _) = directory();
    let base = parent
        .path()
        .join(std::ffi::OsString::from_vec(b"xdg-\xff".to_vec()));
    let state = StateConfig::resolve(None, Some(&base))
        .unwrap()
        .open()
        .unwrap();
    assert_eq!(state.root(), base.join("aerobrowser"));
    assert!(state.acquire_task(&TaskId::from("task")).is_ok());
}

#[test]
fn private_layout_refuses_symlinks_hardlinks_and_permissive_objects() {
    use aero_agent::config::StateConfig;
    use aero_core::ids::RuntimeEpoch;
    let (parent, _) = directory();
    let outside = parent.path().join("outside");
    fs::create_dir(&outside).unwrap();
    let alias = parent.path().join("alias");
    symlink(&outside, &alias).unwrap();
    assert!(matches!(
        StateConfig::resolve(None, Some(&alias)).unwrap().open(),
        Err(StoreError::UnsafePath)
    ));
    assert!(!outside.join("aerobrowser").exists());
    let base = parent.path().join("xdg");
    let config = StateConfig::resolve(None, Some(&base)).unwrap();
    let state = config.open().unwrap();
    let task = state.acquire_task(&TaskId::from("task")).unwrap();
    let task_path = task.directory().to_path_buf();
    drop(task);
    let lock = task_path.join("controller.lock");
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        state.acquire_task(&TaskId::from("task")),
        Err(StoreError::UnsafePath)
    ));
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(&lock, outside.join("hardlink")).unwrap();
    assert!(matches!(
        state.acquire_task(&TaskId::from("task")),
        Err(StoreError::UnsafePath)
    ));
    fs::remove_file(outside.join("hardlink")).unwrap();
    fs::remove_file(&lock).unwrap();
    symlink(outside.join("sentinel"), &lock).unwrap();
    assert!(matches!(
        state.acquire_task(&TaskId::from("task")),
        Err(StoreError::UnsafePath)
    ));
    assert!(!outside.join("sentinel").exists());
    fs::remove_file(&lock).unwrap();
    let task = state.acquire_task(&TaskId::from("task")).unwrap();
    symlink(&outside, task.directory().join("runs")).unwrap();
    assert!(matches!(
        task.create_runtime(&RunId::from("run"), &RuntimeEpoch::from("epoch")),
        Err(StoreError::UnsafePath)
    ));
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
    drop(task);
    drop(state);
    fs::set_permissions(config.root(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(config.open(), Err(StoreError::UnsafePath)));
    assert_eq!(
        fs::metadata(config.root()).unwrap().permissions().mode() & 0o777,
        0o755
    );
}

#[test]
fn private_layout_and_journal_keep_history_across_runtime_generations() {
    use aero_agent::config::StateConfig;
    use aero_core::ids::RuntimeEpoch;
    let (parent, _) = directory();
    let state = StateConfig::resolve(Some(parent.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let (owner, client, context) = seeded_writer(state.journal_directory());
    let task = state.acquire_task(&context.task_id).unwrap();
    let first = task
        .create_runtime(&context.run_id, &RuntimeEpoch::from("first"))
        .unwrap();
    let prepared = client
        .prepare_action(proposal(context.clone()))
        .unwrap()
        .blocking_recv()
        .unwrap();
    client
        .commit_step(commit(prepared, "artifact"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let profile = first.profile_directory().to_path_buf();
    drop(first);
    let second = task
        .create_runtime(&context.run_id, &RuntimeEpoch::from("second"))
        .unwrap();
    assert_ne!(profile, second.profile_directory());
    assert!(profile.exists());
    assert_eq!(
        client
            .load_resume(context)
            .unwrap()
            .blocking_recv()
            .unwrap()
            .usage
            .attempts_spent,
        1
    );
    drop(task);
    drop(second);
    owner.close().unwrap();
}

fn task_contract(task: &str) -> TaskContract {
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
fn run_context(task: &str, run: &str) -> RunContext {
    RunContext {
        task_id: TaskId::from(task),
        run_id: RunId::from(run),
        contract_revision: 1,
    }
}
fn proposal(context: RunContext) -> PrepareAction {
    PrepareAction {
        context,
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
    }
}
fn commit(prepared: PreparedAction, artifact: &str) -> CommitStep {
    CommitStep {
        evidence: vec![EvidenceRecord {
            id: EvidenceId::from(format!(
                "evidence-{}-{}",
                prepared.context.run_id.as_str(),
                prepared.step
            )),
            context: prepared.context.clone(),
            sha256: "a".repeat(64),
        }],
        artifacts: vec![ArtifactRecord {
            id: ArtifactId::from(artifact),
            context: prepared.context.clone(),
            sha256: "b".repeat(64),
            bytes: 42,
        }],
        prepared,
        outcome: StepOutcome {
            schema_version: 1,
            dispatch: DispatchState::Dispatched,
            verification: VerificationStatus::Passed,
        },
        actual: Some(Resources {
            input_tokens: 7,
            output_tokens: 3,
            active_ms: 80,
        }),
    }
}
fn seeded_writer(path: &Path) -> (Journal, JournalClient, RunContext) {
    let owner = Journal::start(path, 8).unwrap();
    let client = owner.client();
    client
        .create_task(task_contract("task"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let context = run_context("task", "run");
    client
        .create_run(context.clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    (owner, client, context)
}

#[test]
fn writer_persists_validated_task_contract() {
    let (_parent, path) = directory();
    let owner = Journal::start(&path, 8).unwrap();
    assert!(
        owner
            .client()
            .create_task(task_contract("task"))
            .unwrap()
            .blocking_recv()
            .is_ok()
    );
    owner.close().unwrap();
    let connection = raw(&path);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM task_contracts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn new_run_retires_old_run_even_after_new_run_settles() {
    let (_parent, path) = directory();
    let (owner, client, old) = seeded_writer(&path);
    let newer = run_context("task", "new-run");
    client
        .create_run(newer.clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    let prepared = client
        .prepare_action(proposal(newer))
        .unwrap()
        .blocking_recv()
        .unwrap();
    client
        .commit_step(commit(prepared, "artifact"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert!(matches!(
        client
            .prepare_action(proposal(old))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::Conflict)
    ));
    owner.close().unwrap();
}

#[test]
fn resume_cannot_reclassify_a_live_prepared_action() {
    let (_parent, path) = directory();
    let (owner, client, context) = seeded_writer(&path);
    let prepared = client
        .prepare_action(proposal(context.clone()))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert!(matches!(
        client.load_resume(context.clone()).unwrap().blocking_recv(),
        Err(StoreError::Conflict)
    ));
    client
        .commit_step(commit(prepared, "artifact"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    owner.close().unwrap();
}

#[test]
fn prepared_orphan_requires_review_across_all_task_runs() {
    let (_parent, path) = directory();
    let (owner, client, context) = seeded_writer(&path);
    drop(client.prepare_action(proposal(context.clone())).unwrap());
    owner.close().unwrap();
    let owner = Journal::start(&path, 8).unwrap();
    let client = owner.client();
    let resume = client
        .load_resume(context.clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert!(resume.needs_review);
    assert_eq!(resume.next_step, 2);
    assert_eq!(resume.usage.attempts_spent, 1);
    assert_eq!(resume.usage.tokens_reserved, 15);
    assert_eq!(
        client
            .load_audit(context.clone(), 0, 10)
            .unwrap()
            .blocking_recv()
            .unwrap()
            .entries[0]
            .state,
        ActionState::Uncertain
    );
    assert!(matches!(
        client
            .prepare_action(proposal(context.clone()))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::NeedsReview)
    ));
    assert!(matches!(
        client
            .create_run(run_context("task", "other-run"))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::NeedsReview)
    ));
    let mut revised = task_contract("task");
    revised.revision = 2;
    assert_eq!(
        client.revise_contract(revised).unwrap().blocking_recv(),
        Err(StoreError::NeedsReview)
    );
    assert_eq!(
        client
            .delete_task(TaskId::from("task"))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::NeedsReview)
    );
    assert_eq!(
        client
            .commit_step(commit(
                PreparedAction {
                    context: context.clone(),
                    step: 1
                },
                "late"
            ))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::Conflict)
    );
    let again = client
        .load_resume(context)
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(again.next_step, 2);
    assert_eq!(again.usage.tokens_reserved, 15);
    owner.close().unwrap();
}

#[test]
fn task_budgets_are_directional_cumulative_and_failure_preserves_cursor() {
    let (_parent, path) = directory();
    let owner = Journal::start(&path, 8).unwrap();
    let client = owner.client();
    let mut contract = task_contract("task");
    contract.budgets.max_input_tokens = 17;
    contract.budgets.max_output_tokens = 8;
    contract.budgets.max_action_attempts = 2;
    contract.budgets.max_active_ms = 180;
    client
        .create_task(contract)
        .unwrap()
        .blocking_recv()
        .unwrap();
    let context = run_context("task", "run");
    client
        .create_run(context.clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    for reserve in [
        Resources {
            input_tokens: 18,
            output_tokens: 0,
            active_ms: 0,
        },
        Resources {
            input_tokens: 0,
            output_tokens: 9,
            active_ms: 0,
        },
        Resources {
            input_tokens: 0,
            output_tokens: 0,
            active_ms: 181,
        },
    ] {
        let mut request = proposal(context.clone());
        request.reserve = reserve;
        assert!(matches!(
            client.prepare_action(request).unwrap().blocking_recv(),
            Err(StoreError::BudgetExceeded)
        ));
    }
    let first = client
        .prepare_action(proposal(context.clone()))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(first.step, 1);
    client
        .commit_step(commit(first, "one"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let newer = run_context("task", "second-run");
    client
        .create_run(newer.clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    let second = client
        .prepare_action(proposal(newer.clone()))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(second.step, 1);
    client
        .commit_step(commit(second, "two"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert!(matches!(
        client
            .prepare_action(proposal(newer.clone()))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::BudgetExceeded)
    ));
    let resume = client.load_resume(newer).unwrap().blocking_recv().unwrap();
    assert_eq!(resume.usage.attempts_spent, 2);
    assert_eq!(resume.usage.tokens_spent, 20);
    assert_eq!(resume.next_step, 2);
    owner.close().unwrap();
}

#[test]
fn revision_keeps_usage_invalidates_grants_and_rejects_stale_runs() {
    let (_parent, path) = directory();
    let (owner, client, context) = seeded_writer(&path);
    let prepared = client
        .prepare_action(proposal(context.clone()))
        .unwrap()
        .blocking_recv()
        .unwrap();
    client
        .commit_step(commit(prepared, "artifact"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    owner.close().unwrap();
    let connection = raw(&path);
    connection
        .execute(
            "INSERT INTO capability_grants VALUES ('grant','task',1,'synthetic','{}','later',0,0)",
            [],
        )
        .unwrap();
    connection.execute("INSERT INTO approvals VALUES ('approval','run','task',1,'obs','digest','{}','https://example.test','later',0)",[]).unwrap();
    drop(connection);
    let owner = Journal::start(&path, 8).unwrap();
    let client = owner.client();
    let mut revised = task_contract("task");
    revised.revision = 2;
    revised.budgets.max_input_tokens = 6;
    assert_eq!(
        client.revise_contract(revised).unwrap().blocking_recv(),
        Err(StoreError::BudgetExceeded)
    );
    let mut revised = task_contract("task");
    revised.revision = 2;
    client
        .revise_contract(revised.clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        client.revise_contract(revised).unwrap().blocking_recv(),
        Err(StoreError::Conflict)
    );
    assert!(matches!(
        client
            .prepare_action(proposal(context.clone()))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::Conflict)
    ));
    let newer = RunContext {
        contract_revision: 2,
        ..run_context("task", "new-run")
    };
    client
        .create_run(newer.clone())
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        client
            .load_resume(newer)
            .unwrap()
            .blocking_recv()
            .unwrap()
            .usage
            .tokens_spent,
        10
    );
    owner.close().unwrap();
    let connection = raw(&path);
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
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM task_contracts", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

#[test]
fn unknown_usage_and_uncertain_effects_never_refund_or_complete() {
    for mode in ["usage", "dispatch", "unchecked"] {
        let (_parent, path) = directory();
        let (owner, client, context) = seeded_writer(&path);
        let mut request = proposal(context.clone());
        request.decision.action = Action::Complete {
            summary: "synthetic proposal".into(),
        };
        let prepared = client
            .prepare_action(request)
            .unwrap()
            .blocking_recv()
            .unwrap();
        let mut result = commit(prepared.clone(), "artifact");
        match mode {
            "usage" => result.actual = None,
            "dispatch" => {
                result.outcome.dispatch = DispatchState::Uncertain;
                result.outcome.verification = VerificationStatus::Inconclusive;
            }
            _ => result.outcome.verification = VerificationStatus::NotChecked,
        }
        client.commit_step(result).unwrap().blocking_recv().unwrap();
        let state = client
            .load_resume(context.clone())
            .unwrap()
            .blocking_recv()
            .unwrap();
        assert!(state.needs_review);
        assert_eq!(state.usage.tokens_reserved, 15);
        assert_eq!(
            client
                .commit_step(commit(prepared, "again"))
                .unwrap()
                .blocking_recv(),
            Err(StoreError::Conflict)
        );
        owner.close().unwrap();
        assert_eq!(
            raw(&path)
                .query_row("SELECT status FROM tasks", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "needs_review"
        );
    }
}

#[test]
fn audit_is_paginated_read_only_and_lost_commit_reply_is_durable() {
    let (_parent, path) = directory();
    let (owner, client, context) = seeded_writer(&path);
    for i in 1..=3 {
        let prepared = client
            .prepare_action(proposal(context.clone()))
            .unwrap()
            .blocking_recv()
            .unwrap();
        drop(
            client
                .commit_step(commit(prepared, &format!("artifact-{i}")))
                .unwrap(),
        );
    }
    let first = client
        .load_audit(context.clone(), 0, 2)
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        first.entries.iter().map(|e| e.step).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(first.has_more);
    let last = client
        .load_audit(context.clone(), 2, 2)
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(
        last.entries.iter().map(|e| e.step).collect::<Vec<_>>(),
        vec![3]
    );
    assert!(!last.has_more);
    assert!(matches!(
        client.load_audit(context.clone(), 0, 101),
        Err(StoreError::InvalidInput)
    ));
    owner.close().unwrap();
    let before = fs::read(path.join("journal.sqlite3")).unwrap();
    let owner = Journal::start(&path, 8).unwrap();
    let client = owner.client();
    assert_eq!(
        client
            .load_resume(context.clone())
            .unwrap()
            .blocking_recv()
            .unwrap()
            .usage
            .attempts_spent,
        3
    );
    assert_eq!(
        client
            .load_audit(context, 0, 100)
            .unwrap()
            .blocking_recv()
            .unwrap()
            .entries
            .len(),
        3
    );
    owner.close().unwrap();
    assert_eq!(fs::read(path.join("journal.sqlite3")).unwrap(), before);
}

#[test]
fn cross_owner_receipts_and_overruns_cannot_settle_or_consume_steps() {
    let (_parent, path) = directory();
    let (owner, client, context) = seeded_writer(&path);
    let prepared = client
        .prepare_action(proposal(context.clone()))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let mut wrong = commit(prepared.clone(), "artifact");
    wrong.evidence[0].context.task_id = TaskId::from("other");
    assert!(matches!(
        client.commit_step(wrong).unwrap().blocking_recv(),
        Err(StoreError::OwnershipMismatch)
    ));
    let mut wrong = commit(prepared.clone(), "artifact");
    wrong.prepared.context.task_id = TaskId::from("other");
    wrong.evidence.clear();
    wrong.artifacts.clear();
    assert_eq!(
        client.commit_step(wrong).unwrap().blocking_recv(),
        Err(StoreError::OwnershipMismatch)
    );
    let mut over = commit(prepared.clone(), "artifact");
    over.actual.as_mut().unwrap().input_tokens = 11;
    assert_eq!(
        client.commit_step(over).unwrap().blocking_recv(),
        Err(StoreError::BudgetExceeded)
    );
    assert_eq!(
        client
            .load_audit(context.clone(), 0, 10)
            .unwrap()
            .blocking_recv()
            .unwrap()
            .entries[0]
            .state,
        ActionState::Uncertain
    );
    assert_eq!(
        client
            .commit_step(commit(prepared, "again"))
            .unwrap()
            .blocking_recv(),
        Err(StoreError::Conflict)
    );
    let resume = client
        .load_resume(context)
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert!(resume.needs_review);
    assert_eq!(resume.next_step, 2);
    owner.close().unwrap();
}

#[test]
fn delete_task_removes_only_its_sql_records_and_closes_all_clients() {
    let (_parent, path) = directory();
    let (owner, client, context) = seeded_writer(&path);
    let prepared = client
        .prepare_action(proposal(context))
        .unwrap()
        .blocking_recv()
        .unwrap();
    client
        .commit_step(commit(prepared, "artifact"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    client
        .create_task(task_contract("other"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    client
        .delete_task(TaskId::from("task"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    owner.close().unwrap();
    assert!(matches!(
        client.delete_task(TaskId::from("other")),
        Err(StoreError::WorkerClosed)
    ));
    let connection = raw(&path);
    for table in [
        "sessions",
        "actions",
        "budget_reservations",
        "checkpoints",
        "evidence",
        "artifacts",
    ] {
        assert_eq!(
            connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
    assert_eq!(
        connection
            .query_row("SELECT id FROM tasks", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "other"
    );
}

#[test]
fn step_and_checkpoint_are_atomic() {
    let (_parent, path) = directory();
    let (owner, client, context) = seeded_writer(&path);
    let first = client
        .prepare_action(proposal(context.clone()))
        .unwrap()
        .blocking_recv()
        .unwrap();
    client
        .commit_step(commit(first, "artifact-existing"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let second = client
        .prepare_action(proposal(context.clone()))
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(second.step, 2);
    // The artifact uniqueness failure occurs after action/usage/evidence writes.
    assert!(
        client
            .commit_step(commit(second.clone(), "artifact-existing"))
            .unwrap()
            .blocking_recv()
            .is_err()
    );
    let audit = client
        .load_audit(context.clone(), 0, 100)
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert_eq!(audit.entries.len(), 2);
    assert_eq!(audit.entries[1].state, ActionState::Prepared);
    assert!(audit.entries[1].outcome.is_none());
    // Retrying the local transaction after a definite rollback is permitted;
    // this is NOT permission to repeat an external action.
    client
        .commit_step(commit(second, "artifact-new"))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let resume = client
        .load_resume(context)
        .unwrap()
        .blocking_recv()
        .unwrap();
    assert!(!resume.needs_review);
    assert_eq!(resume.next_step, 3);
    assert_eq!(resume.usage.attempts_spent, 2);
    assert_eq!(resume.usage.tokens_spent, 20);
    assert_eq!(resume.usage.tokens_reserved, 0);
    assert_eq!(resume.usage.active_ms_spent, 160);
    let checkpoint = resume.checkpoint.unwrap();
    assert_eq!(checkpoint.step, 2);
    assert_eq!(checkpoint.next_step, 3);
    owner.close().unwrap();
    let connection = raw(&path);
    for table in ["actions", "checkpoints", "evidence", "artifacts"] {
        assert_eq!(
            connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            2
        );
    }
}

fn directory() -> (tempfile::TempDir, PathBuf) {
    let parent = tempfile::tempdir().unwrap();
    fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let path = parent.path().join("private-store");
    (parent, path)
}
fn raw(path: &Path) -> Connection {
    // Trusted test-only SQL bypass, never exposed by Store to production adapters.
    let connection = Connection::open(path.join("journal.sqlite3")).unwrap();
    connection
        .pragma_update(None, "foreign_keys", true)
        .unwrap();
    connection
}
fn seed(connection: &Connection, task: &str, run: &str) {
    connection
        .execute(
            "INSERT INTO tasks(id,status,created_at,updated_at) VALUES (?1,'draft','now','now')",
            [task],
        )
        .unwrap();
    connection.execute("INSERT INTO task_contracts(task_id,revision,schema_version,contract_json,created_at) VALUES (?1,1,1,'{}','now')", [task]).unwrap();
    connection.execute("INSERT INTO sessions(id,task_id,contract_revision,goal,status,created_at,updated_at) VALUES (?1,?2,1,'synthetic','paused','now','now')", params![run,task]).unwrap();
}
fn ready() -> (tempfile::TempDir, PathBuf, Connection) {
    let (parent, path) = directory();
    drop(Store::open(&path).unwrap());
    let connection = raw(&path);
    (parent, path, connection)
}

#[test]
fn schema_and_foreign_keys() {
    let (_parent, path) = directory();
    let store = Store::open(&path).unwrap();
    let settings = store.settings().unwrap();
    assert_eq!(settings.schema_version, 1);
    assert!(settings.foreign_keys);
    assert_eq!(settings.journal_mode, "wal");
    assert_eq!(settings.synchronous, 2);
    assert_eq!(settings.busy_timeout_ms, 5000);
    drop(store);
    let connection = raw(&path);
    let tables: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(tables, 12);
    seed(&connection, "task-a", "run-a");
    assert!(connection.execute("INSERT INTO sessions(id,task_id,contract_revision,goal,status,created_at,updated_at) VALUES ('orphan','missing',1,'test','paused','now','now')", []).is_err());
    assert!(connection.execute("INSERT INTO tasks(id,status,created_at,updated_at) VALUES ('task-a','draft','now','now')", []).is_err());
    assert!(connection.execute("INSERT INTO task_contracts(task_id,revision,schema_version,contract_json,created_at) VALUES ('task-a',1,1,'{}','now')", []).is_err());
    assert!(
        connection
            .execute(
                "UPDATE task_contracts SET contract_json='{}' WHERE task_id='task-a'",
                []
            )
            .is_err()
    );
}

#[test]
fn store_reopens_without_losing_rows_or_changing_contracts() {
    let (_parent, path, connection) = ready();
    seed(&connection, "task-a", "run-a");
    drop(connection);
    let store = Store::open(&path).unwrap();
    assert_eq!(store.settings().unwrap().schema_version, 1);
    drop(store);
    let connection = raw(&path);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM sessions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        connection
            .query_row("SELECT contract_json FROM task_contracts", [], |row| row
                .get::<_, String>(
                0
            ))
            .unwrap(),
        "{}"
    );
}

#[test]
fn schema_enforces_cross_task_and_revision_ownership() {
    let (_parent, _path, connection) = ready();
    seed(&connection, "task-a", "run-a");
    seed(&connection, "task-b", "run-b");
    for sql in [
        "INSERT INTO budget_reservations(id,task_id,session_id,reservation_json,state,created_at) VALUES ('res','task-b','run-a','{}','reserved','now')",
        "INSERT INTO evidence VALUES ('ev','task-b','run-a','{}','now')",
        "INSERT INTO artifacts VALUES ('art','task-b','run-a','{}','now')",
        "INSERT INTO approvals VALUES ('approval','run-a','task-b',1,'obs','digest','{}','https://example.test','later',0)",
        "INSERT INTO criterion_results VALUES ('task-b',1,'criterion','run-a','not_checked','{}')",
    ] {
        assert!(
            connection.execute(sql, []).is_err(),
            "accepted cross-owner row: {sql}"
        );
    }
    connection
        .execute(
            "INSERT INTO task_contracts VALUES ('task-a',2,1,'{}','now')",
            [],
        )
        .unwrap();
    assert!(connection.execute("INSERT INTO criterion_results VALUES ('task-a',2,'criterion','run-a','not_checked','{}')", []).is_err());
    assert!(connection.execute("INSERT INTO approvals VALUES ('approval','run-a','task-a',2,'obs','digest','{}','https://example.test','later',0)", []).is_err());
}

#[test]
fn schema_rejects_invalid_types_counters_json_and_ciphertext_shapes() {
    let (_parent, _path, connection) = ready();
    seed(&connection, "task-a", "run-a");
    for sql in [
        "UPDATE tasks SET attempts_spent=-1",
        "UPDATE tasks SET tokens_reserved=1.5",
        "UPDATE tasks SET active_ms_spent='not-an-integer'",
        "UPDATE tasks SET tokens_spent=9223372036854775807,tokens_reserved=1",
        "UPDATE sessions SET next_step=0",
        "UPDATE tasks SET status='model_says_done'",
        "INSERT INTO tasks(id,status,created_at,updated_at) VALUES(NULL,'draft','now','now')",
        "INSERT INTO tasks(id,status,created_at,updated_at) VALUES('','draft','now','now')",
        "INSERT INTO task_contracts VALUES ('task-a',2,1,'not json','now')",
        "INSERT INTO task_contracts VALUES ('task-a',2,2,'{}','now')",
        "INSERT INTO actions VALUES ('run-a',0,'obs','{}','prepared','public',NULL,'now')",
        "INSERT INTO checkpoints VALUES ('run-a',0,1,'{}',NULL,X'01',NULL,'now')",
        "INSERT INTO checkpoints VALUES ('run-a',0,1,'{}',X'',X'', '', 'now')",
    ] {
        assert!(
            connection.execute(sql, []).is_err(),
            "accepted invalid row: {sql}"
        );
    }
    connection
        .execute(
            "INSERT INTO checkpoints VALUES ('run-a',0,1,'{}',NULL,NULL,NULL,'now')",
            [],
        )
        .unwrap();
}

#[test]
fn grant_payload_identity_and_expiry_are_immutable() {
    let (_parent, _path, connection) = ready();
    seed(&connection, "task-a", "run-a");
    connection.execute("INSERT INTO capability_grants VALUES ('grant','task-a',1,'trusted-user','{}','later',0,0)", []).unwrap();
    for field in [
        "id",
        "task_id",
        "contract_revision",
        "issued_by",
        "grant_json",
        "expires_at",
    ] {
        assert!(
            connection
                .execute(&format!("UPDATE capability_grants SET {field}={field}"), [])
                .is_err(),
            "mutable {field}"
        );
    }
    connection
        .execute("UPDATE capability_grants SET revoked=1,quota_used=1", [])
        .unwrap();
}

#[test]
fn deleting_a_task_cascades_only_its_owned_records() {
    let (_parent, _path, connection) = ready();
    seed(&connection, "task-a", "run-a");
    seed(&connection, "task-b", "run-b");
    connection.execute_batch("INSERT INTO actions VALUES ('run-a',1,'obs','{}','prepared','public',NULL,'now');
        INSERT INTO budget_reservations(id,task_id,session_id,reservation_json,state,created_at) VALUES ('res','task-a','run-a','{}','reserved','now');
        INSERT INTO evidence VALUES ('ev','task-a','run-a','{}','now');
        INSERT INTO artifacts VALUES ('art','task-a','run-a','{}','now');
        INSERT INTO checkpoints VALUES ('run-a',0,1,'{}',NULL,NULL,NULL,'now');
        INSERT INTO capability_grants VALUES ('grant','task-a',1,'trusted-user','{}','later',0,0);
        INSERT INTO approvals VALUES ('approval','run-a','task-a',1,'obs','digest','{}','https://example.test','later',0);
        INSERT INTO criterion_results VALUES ('task-a',1,'criterion','run-a','not_checked','{}');
        DELETE FROM tasks WHERE id='task-a';").unwrap();
    for table in [
        "actions",
        "budget_reservations",
        "evidence",
        "artifacts",
        "checkpoints",
        "capability_grants",
        "approvals",
        "criterion_results",
    ] {
        assert_eq!(
            connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row
                    .get::<_, i64>(0))
                .unwrap(),
            0,
            "{table}"
        );
    }
    assert_eq!(
        connection
            .query_row("SELECT id FROM tasks", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "task-b"
    );
    assert_eq!(
        connection
            .query_row("SELECT id FROM sessions", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "run-b"
    );
}

#[test]
fn open_refuses_unknown_foreign_partial_and_corrupt_databases() {
    for version in [2, 999] {
        let (_parent, path, connection) = ready();
        connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        drop(connection);
        assert!(matches!(
            Store::open(&path),
            Err(StoreError::UnsupportedVersion)
        ));
        assert_eq!(
            raw(&path)
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .unwrap(),
            version
        );
        // A refused open must release its owner lock, not strand the journal.
        raw(&path).pragma_update(None, "user_version", 1).unwrap();
        assert!(Store::open(&path).is_ok());
    }
    let (_parent, path, connection) = ready();
    connection
        .execute_batch("DROP TRIGGER immutable_contract")
        .unwrap();
    drop(connection);
    assert!(matches!(
        Store::open(&path),
        Err(StoreError::SchemaMismatch)
    ));
    let (_parent, path, connection) = ready();
    connection
        .pragma_update(None, "application_id", 1234)
        .unwrap();
    drop(connection);
    assert!(matches!(
        Store::open(&path),
        Err(StoreError::ForeignDatabase)
    ));
    let (_parent, path, connection) = ready();
    connection.pragma_update(None, "user_version", 0).unwrap();
    drop(connection);
    assert!(matches!(
        Store::open(&path),
        Err(StoreError::SchemaMismatch)
    ));
    let (_parent, path) = directory();
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let file = path.join("journal.sqlite3");
    fs::write(&file, b"SYNTHETIC_PRIVATE_CORRUPTION").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    let error = Store::open(&path).err().expect("corruption refused");
    assert_eq!(error, StoreError::CorruptDatabase);
    assert!(!format!("{error:?} {error}").contains("SYNTHETIC"));
    assert_eq!(fs::read(&file).unwrap(), b"SYNTHETIC_PRIVATE_CORRUPTION");
}

#[test]
fn store_paths_and_sidecars_are_private_and_single_owner() {
    let (_parent, path) = directory();
    let store = Store::open(&path).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o700
    );
    for entry in fs::read_dir(&path).unwrap() {
        assert_eq!(
            entry.unwrap().metadata().unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    assert!(matches!(Store::open(&path), Err(StoreError::Busy)));
    drop(store);
    assert!(Store::open(&path).is_ok());
}

#[test]
fn open_rejects_symlinks_hardlinks_and_permissive_storage() {
    let (parent, path) = directory();
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(Store::open(&path), Err(StoreError::UnsafePath)));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o755
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let link = parent.path().join("alias");
    symlink(&path, &link).unwrap();
    assert!(matches!(Store::open(&link), Err(StoreError::UnsafePath)));
    assert!(matches!(
        Store::open(&link.join("child")),
        Err(StoreError::UnsafePath)
    ));
    let outside = parent.path().join("outside");
    fs::write(&outside, b"sentinel").unwrap();
    symlink(&outside, path.join("journal.sqlite3")).unwrap();
    assert!(matches!(Store::open(&path), Err(StoreError::UnsafePath)));
    fs::remove_file(path.join("journal.sqlite3")).unwrap();
    fs::hard_link(&outside, path.join("journal.sqlite3")).unwrap();
    assert!(matches!(Store::open(&path), Err(StoreError::UnsafePath)));
    assert_eq!(fs::read(outside).unwrap(), b"sentinel");
    let (_parent, path, connection) = ready();
    drop(connection);
    fs::set_permissions(
        path.join("journal.sqlite3"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(matches!(Store::open(&path), Err(StoreError::UnsafePath)));
    let (parent, path, connection) = ready();
    drop(connection);
    let outside = parent.path().join("outside");
    fs::write(&outside, b"sentinel").unwrap();
    symlink(&outside, path.join("journal.sqlite3-wal")).unwrap();
    assert!(matches!(Store::open(&path), Err(StoreError::UnsafePath)));
    assert_eq!(fs::read(outside).unwrap(), b"sentinel");
}

#[test]
fn schema_identity_does_not_treat_underscore_as_a_wildcard() {
    let (_parent, path, connection) = ready();
    connection
        .execute_batch("CREATE TABLE sqliteXforeign(id TEXT)")
        .unwrap();
    drop(connection);
    assert!(matches!(
        Store::open(&path),
        Err(StoreError::SchemaMismatch)
    ));
}

#[test]
fn store_refuses_nonsticky_writable_ancestors_before_creating_leaf() {
    let parent = tempfile::tempdir().unwrap();
    fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let shared = parent.path().join("shared");
    fs::create_dir(&shared).unwrap();
    fs::set_permissions(&shared, fs::Permissions::from_mode(0o777)).unwrap();
    let path = shared.join("store");
    assert!(matches!(Store::open(&path), Err(StoreError::UnsafePath)));
    assert!(!path.exists());
}

#[test]
fn immutable_revisions_cannot_be_replaced_or_individually_deleted() {
    let (_parent, _path, connection) = ready();
    seed(&connection, "task-a", "run-a");
    for sql in [
        "INSERT OR REPLACE INTO task_contracts VALUES ('task-a',1,1,'{}','later')",
        "DELETE FROM task_contracts WHERE task_id='task-a'",
        "INSERT OR REPLACE INTO tasks(id,status,created_at,updated_at) VALUES ('task-a','draft','later','later')",
        "UPDATE sessions SET task_id=task_id",
    ] {
        assert!(connection.execute(sql, []).is_err());
    }
}

#[test]
fn owner_lock_excludes_another_process_and_releases_on_drop() {
    const CHILD_PATH: &str = "AERO_B04_TEST_LOCK_PATH";
    if let Some(path) = std::env::var_os(CHILD_PATH) {
        let expected_busy = std::env::var_os("AERO_B04_TEST_EXPECT_BUSY").is_some();
        let result = Store::open(Path::new(&path));
        if expected_busy {
            assert!(matches!(result, Err(StoreError::Busy)));
        } else {
            assert!(result.is_ok());
        }
        return;
    }
    let (_parent, path) = directory();
    let store = Store::open(&path).unwrap();
    let child = |busy: bool| {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "owner_lock_excludes_another_process_and_releases_on_drop",
            ])
            .env(CHILD_PATH, &path)
            .env_remove("AERO_B04_TEST_EXPECT_BUSY");
        if busy {
            command.env("AERO_B04_TEST_EXPECT_BUSY", "1");
        }
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    };
    child(true);
    drop(store);
    child(false);
}

#[test]
fn reopen_detects_foreign_key_corruption_even_when_integrity_is_ok() {
    let (_parent, path, connection) = ready();
    connection
        .pragma_update(None, "foreign_keys", false)
        .unwrap();
    connection
        .execute(
            "INSERT INTO evidence VALUES ('orphan','missing-task','missing-run','{}','now')",
            [],
        )
        .unwrap();
    assert_eq!(
        connection
            .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    drop(connection);
    assert!(matches!(
        Store::open(&path),
        Err(StoreError::CorruptDatabase)
    ));
}

#[test]
fn every_auxiliary_file_is_checked_before_sqlite_opens() {
    for name in [
        "journal.lock",
        "journal.sqlite3-wal",
        "journal.sqlite3-shm",
        "journal.sqlite3-journal",
    ] {
        for kind in ["symlink", "hardlink", "permissive", "directory"] {
            let (parent, path, connection) = ready();
            drop(connection);
            let database_bytes = fs::read(path.join("journal.sqlite3")).unwrap();
            let target = path.join(name);
            if target.exists() {
                fs::remove_file(&target).unwrap();
            }
            let outside = parent.path().join("outside");
            fs::write(&outside, b"synthetic sentinel").unwrap();
            fs::set_permissions(&outside, fs::Permissions::from_mode(0o600)).unwrap();
            match kind {
                "symlink" => symlink(&outside, &target).unwrap(),
                "hardlink" => fs::hard_link(&outside, &target).unwrap(),
                "permissive" => {
                    fs::write(&target, b"synthetic sentinel").unwrap();
                    fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();
                }
                "directory" => fs::create_dir(&target).unwrap(),
                _ => unreachable!(),
            }
            assert!(
                matches!(Store::open(&path), Err(StoreError::UnsafePath)),
                "{name}: {kind}"
            );
            assert_eq!(fs::read(&outside).unwrap(), b"synthetic sentinel");
            assert_eq!(
                fs::read(path.join("journal.sqlite3")).unwrap(),
                database_bytes
            );
        }
    }
}
