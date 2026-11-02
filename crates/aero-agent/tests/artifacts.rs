#![cfg(target_os = "linux")]

use aero_agent::{
    artifacts::{
        ArtifactPersistence, ArtifactReconciliation, ArtifactStore, MAX_ARTIFACT_BYTES,
        MAX_RECONCILE_ENTRIES,
    },
    config::StateConfig,
    store::StoreError,
    store::worker::{
        ArtifactRecord, CommitStep, Journal, MAX_ARTIFACT_INVENTORY, PrepareAction, PreparedAction,
        Resources, StepOutcome,
    },
    tasks::TaskAuthority,
};
use aero_core::actions::{Action, Decision};
use aero_core::checkpoint::RunContext;
use aero_core::events::{DispatchState, VerificationStatus};
use aero_core::ids::{ArtifactId, ObservationId, RunId, TaskId};
use aero_core::task::{StartPrerequisites, TaskContract};
use std::os::unix::fs::{PermissionsExt, symlink};

fn private_tempdir() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    home
}

#[test]
fn unclassified_and_sensitive_bytes_are_not_persisted() {
    let home = private_tempdir();
    let state = StateConfig::resolve(Some(home.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let lease = state.acquire_task(&TaskId::from("task")).unwrap();
    let store = ArtifactStore::open(&lease).unwrap();
    for policy in [ArtifactPersistence::Unknown, ArtifactPersistence::Sensitive] {
        assert!(matches!(
            store.stage(policy, b"do not persist"),
            Err(StoreError::EncryptionRequired)
        ));
    }
    assert_eq!(
        std::fs::read_dir(lease.directory().join("artifacts"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn stages_real_bytes_and_reopens_for_verified_readback() {
    let home = private_tempdir();
    let state = StateConfig::resolve(Some(home.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let lease = state.acquire_task(&TaskId::from("task-a")).unwrap();
    let path = lease.directory().join("artifacts");
    let store = ArtifactStore::open(&lease).unwrap();
    let bytes = b"actual\0artifact bytes\xff";
    let artifact = store.stage(ArtifactPersistence::Public, bytes).unwrap();
    assert_eq!(artifact.len(), bytes.len());
    assert_eq!(
        artifact.sha256(),
        "8a85d0d1ba9df7d06426333de7ad77b7a96abc8fc975cd433ace414c1de88683"
    );
    assert_eq!(
        std::fs::read(path.join(artifact.id().as_str())).unwrap(),
        bytes
    );
    assert_eq!(store.readback(&artifact).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(path.join(artifact.id().as_str()))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    let second = store.stage(ArtifactPersistence::Public, bytes).unwrap();
    assert_ne!(artifact.id(), second.id());
    assert!(matches!(
        store.stage(
            ArtifactPersistence::Public,
            &vec![0; MAX_ARTIFACT_BYTES + 1]
        ),
        Err(StoreError::BudgetExceeded)
    ));
    assert!(store.stage(ArtifactPersistence::Public, &[]).is_ok());
    let at_limit = vec![7; MAX_ARTIFACT_BYTES];
    let full = store.stage(ArtifactPersistence::Public, &at_limit).unwrap();
    assert_eq!(store.readback(&full).unwrap(), at_limit);
}

#[test]
fn tamper_missing_symlink_hardlink_and_permissions_fail_closed() {
    let home = private_tempdir();
    let state = StateConfig::resolve(Some(home.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let lease = state.acquire_task(&TaskId::from("task-a")).unwrap();
    let path = lease.directory().join("artifacts");
    let store = ArtifactStore::open(&lease).unwrap();
    let artifact = store
        .stage(ArtifactPersistence::Public, b"original")
        .unwrap();
    let file = path.join(artifact.id().as_str());
    std::fs::write(&file, b"modified").unwrap();
    assert!(store.readback(&artifact).is_err());
    std::fs::write(&file, vec![0; MAX_ARTIFACT_BYTES + 1]).unwrap();
    assert!(store.readback(&artifact).is_err());
    std::fs::remove_file(&file).unwrap();
    assert!(store.readback(&artifact).is_err());
    let target = home.path().join("target");
    std::fs::write(&target, b"original").unwrap();
    symlink(&target, &file).unwrap();
    assert!(store.readback(&artifact).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"original");
    std::fs::remove_file(&file).unwrap();
    std::fs::write(&file, b"original").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(store.readback(&artifact).is_err());
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::hard_link(&file, path.join("hardlink")).unwrap();
    assert!(store.readback(&artifact).is_err());
}

#[test]
fn task_scope_and_lock_are_retained_by_store() {
    let home = private_tempdir();
    let state = StateConfig::resolve(Some(home.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let a = state.acquire_task(&TaskId::from("a/../b")).unwrap();
    let b = state.acquire_task(&TaskId::from("b")).unwrap();
    let a_store = ArtifactStore::open(&a).unwrap();
    let b_store = ArtifactStore::open(&b).unwrap();
    let artifact = a_store
        .stage(ArtifactPersistence::Public, b"private")
        .unwrap();
    // Even if an identically named file is planted in the other task.
    std::fs::copy(
        a.directory().join("artifacts").join(artifact.id().as_str()),
        b.directory().join("artifacts").join(artifact.id().as_str()),
    )
    .unwrap();
    assert!(matches!(
        b_store.readback(&artifact),
        Err(StoreError::OwnershipMismatch)
    ));
    drop(a);
    assert!(matches!(
        state.acquire_task(&TaskId::from("a/../b")),
        Err(StoreError::Busy)
    ));
    assert_eq!(a_store.readback(&artifact).unwrap(), b"private");
    drop(a_store);
    assert!(state.acquire_task(&TaskId::from("a/../b")).is_ok());
}

#[test]
fn refuses_symlink_artifact_directory() {
    let home = private_tempdir();
    let state = StateConfig::resolve(Some(home.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let lease = state.acquire_task(&TaskId::from("task")).unwrap();
    symlink(home.path(), lease.directory().join("artifacts")).unwrap();
    assert!(matches!(
        ArtifactStore::open(&lease),
        Err(StoreError::UnsafePath)
    ));
}

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
        run_id: RunId::from("run"),
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
fn commit(prepared: PreparedAction, artifacts: Vec<ArtifactRecord>) -> CommitStep {
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
        evidence: vec![],
        artifacts,
    }
}
fn started(state: &aero_agent::config::PrivateState, task: &str) -> Journal {
    let journal = Journal::start(state.journal_directory(), 8).unwrap();
    let owner = TaskAuthority::new(&journal).trusted_user();
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
    journal
}

#[test]
fn journal_artifact_records_must_name_task_staged_files() {
    let home = private_tempdir();
    let state = StateConfig::resolve(Some(home.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let journal = started(&state, "owned");
    let context = run("owned");
    let prepared = prepare(&journal, &context);
    let client = journal.client();
    let other_lease = state.acquire_task(&TaskId::from("other")).unwrap();
    let foreign = ArtifactStore::open(&other_lease)
        .unwrap()
        .stage(ArtifactPersistence::Public, b"other task")
        .unwrap();
    let staged_record = |id: &str| ArtifactRecord {
        id: ArtifactId::from(id),
        context: context.clone(),
        sha256: "b".repeat(64),
        bytes: 1,
    };
    for (record, expected) in [
        (staged_record("artifact"), StoreError::InvalidInput),
        (staged_record("../escape"), StoreError::InvalidInput),
        (
            staged_record(&format!("{}x", foreign.id().as_str())),
            StoreError::InvalidInput,
        ),
        (
            staged_record(foreign.id().as_str()),
            StoreError::OwnershipMismatch,
        ),
        (
            foreign.record(context.clone()),
            StoreError::OwnershipMismatch,
        ),
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
    // Rejected attachments leave the action prepared; a real owned file commits.
    let lease = state.acquire_task(&TaskId::from("owned")).unwrap();
    let owned = ArtifactStore::open(&lease)
        .unwrap()
        .stage(ArtifactPersistence::Public, b"owned bytes")
        .unwrap();
    client
        .commit_step(commit(prepared, vec![owned.record(context.clone())]))
        .unwrap()
        .blocking_recv()
        .unwrap();
    journal.close().unwrap();
}

#[test]
fn committed_artifact_reads_back_against_its_journal_record() {
    let home = private_tempdir();
    let state = StateConfig::resolve(Some(home.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let journal = started(&state, "owned");
    let context = run("owned");
    let lease = state.acquire_task(&TaskId::from("owned")).unwrap();
    let store = ArtifactStore::open(&lease).unwrap();
    let staged = store
        .stage(ArtifactPersistence::Public, b"supplier,price\nacme,10\n")
        .unwrap();
    let missing = store
        .stage(ArtifactPersistence::Public, b"later removed")
        .unwrap();
    let client = journal.client();
    let prepared = prepare(&journal, &context);
    client
        .commit_step(commit(
            prepared,
            vec![
                staged.record(context.clone()),
                missing.record(context.clone()),
            ],
        ))
        .unwrap()
        .blocking_recv()
        .unwrap();

    let load = |id: &ArtifactId| {
        client
            .load_artifact(context.task_id.clone(), id.clone())
            .unwrap()
            .blocking_recv()
    };
    let record = load(staged.id()).unwrap();
    assert_eq!(record.id, *staged.id());
    assert!(record.context == context);
    assert_eq!(record.sha256, staged.sha256());
    assert_eq!(record.bytes, staged.len() as u64);
    // Readback is checked against the durable journal record, not the receipt.
    assert_eq!(store.verify(&record).unwrap(), b"supplier,price\nacme,10\n");
    assert_eq!(
        load(&ArtifactId::from("absent")).err(),
        Some(StoreError::NotFound)
    );
    assert_eq!(
        client
            .load_artifact(TaskId::from("other"), staged.id().clone())
            .unwrap()
            .blocking_recv()
            .err(),
        Some(StoreError::NotFound)
    );

    // A journal record cannot vouch for different, absent or foreign bytes.
    let mut forged = record.clone();
    forged.sha256 = "0".repeat(64);
    assert_eq!(store.verify(&forged).err(), Some(StoreError::Conflict));
    let mut resized = record.clone();
    resized.bytes += 1;
    assert_eq!(store.verify(&resized).err(), Some(StoreError::Conflict));
    let mut foreign = record.clone();
    foreign.context = run("other");
    assert_eq!(
        store.verify(&foreign).err(),
        Some(StoreError::OwnershipMismatch)
    );
    let path = lease.directory().join("artifacts");
    std::fs::remove_file(path.join(missing.id().as_str())).unwrap();
    let missing_record = load(missing.id()).unwrap();
    assert_eq!(
        store.verify(&missing_record).err(),
        Some(StoreError::NotFound)
    );
    std::fs::write(
        path.join(staged.id().as_str()),
        b"supplier,price\nacme,99\n",
    )
    .unwrap();
    assert_eq!(store.verify(&record).err(), Some(StoreError::Conflict));
    journal.close().unwrap();
}

#[test]
fn committed_artifact_records_are_immutable_and_corruption_is_refused() {
    let home = private_tempdir();
    let state = StateConfig::resolve(Some(home.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let journal = started(&state, "owned");
    let context = run("owned");
    let lease = state.acquire_task(&TaskId::from("owned")).unwrap();
    let staged = ArtifactStore::open(&lease)
        .unwrap()
        .stage(ArtifactPersistence::Public, b"bytes")
        .unwrap();
    let prepared = prepare(&journal, &context);
    journal
        .client()
        .commit_step(commit(prepared, vec![staged.record(context.clone())]))
        .unwrap()
        .blocking_recv()
        .unwrap();
    journal.close().unwrap();

    let sql =
        rusqlite::Connection::open(state.journal_directory().join("journal.sqlite3")).unwrap();
    let id = staged.id().as_str();
    let forged = format!(
        r#"{{"id":"{id}","context":{{"task_id":"owned","run_id":"run","contract_revision":1}},"sha256":"{}","bytes":5}}"#,
        "0".repeat(64)
    );
    // Neither in-place update, replacement nor individual deletion may rewrite
    // the record a later completion check reads back against.
    assert!(
        sql.execute(
            "UPDATE artifacts SET metadata_json=?2 WHERE id=?1",
            rusqlite::params![id, forged]
        )
        .is_err()
    );
    assert!(
        sql.execute(
            "INSERT OR REPLACE INTO artifacts VALUES (?1,'owned','run',?2,'now')",
            rusqlite::params![id, forged]
        )
        .is_err()
    );
    assert!(
        sql.execute("DELETE FROM artifacts WHERE id=?1", [id])
            .is_err()
    );
    // A malformed or misattributed row is corruption, never a usable record.
    let other = format!("a1-{}-{}", &id[3..67], "f".repeat(32));
    let wrong_task = forged
        .replace(id, &other)
        .replace(r#""task_id":"owned""#, r#""task_id":"other""#);
    sql.execute(
        "INSERT INTO artifacts VALUES (?1,'owned','run',?2,'now')",
        rusqlite::params![other, wrong_task],
    )
    .unwrap();
    drop(sql);

    let journal = Journal::start(state.journal_directory(), 8).unwrap();
    let client = journal.client();
    let load = |id: &str| {
        client
            .load_artifact(TaskId::from("owned"), ArtifactId::from(id))
            .unwrap()
            .blocking_recv()
    };
    assert_eq!(load(id).unwrap().sha256, staged.sha256());
    assert_eq!(load(&other).err(), Some(StoreError::CorruptDatabase));
    journal.close().unwrap();
}

#[test]
fn reconciliation_reports_unrecorded_missing_and_mismatched_files() {
    let home = private_tempdir();
    let state = StateConfig::resolve(Some(home.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let journal = started(&state, "owned");
    let context = run("owned");
    let lease = state.acquire_task(&TaskId::from("owned")).unwrap();
    let store = ArtifactStore::open(&lease).unwrap();
    let stage = |bytes: &[u8]| store.stage(ArtifactPersistence::Public, bytes).unwrap();
    let (kept, removed, tampered) = (stage(b"kept"), stage(b"removed"), stage(b"tampered"));
    let client = journal.client();
    client
        .commit_step(commit(
            prepare(&journal, &context),
            vec![
                kept.record(context.clone()),
                removed.record(context.clone()),
                tampered.record(context.clone()),
            ],
        ))
        .unwrap()
        .blocking_recv()
        .unwrap();
    let records = |task: &str| {
        client
            .load_artifacts(TaskId::from(task))
            .unwrap()
            .blocking_recv()
            .unwrap()
    };
    let recorded = records("owned");
    assert_eq!(recorded.len(), 3);
    assert!(records("other").is_empty());
    let mut all = vec![
        kept.id().clone(),
        removed.id().clone(),
        tampered.id().clone(),
    ];
    all.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    let clean = store.reconcile(&recorded).unwrap();
    assert!(clean.is_consistent());
    assert_eq!(
        clean,
        ArtifactReconciliation {
            verified: all,
            ..ArtifactReconciliation::default()
        }
    );

    // Crash between staging and commit, lost file, modified bytes, and foreign
    // entries are each reported. Nothing is deleted or repaired.
    let orphan = stage(b"staged but never committed");
    let dir = lease.directory().join("artifacts");
    std::fs::remove_file(dir.join(removed.id().as_str())).unwrap();
    std::fs::write(dir.join(tampered.id().as_str()), b"TAMPERED").unwrap();
    std::fs::write(dir.join("notes.txt"), b"not an artifact").unwrap();
    let foreign_name = format!("a1-{}-{}", "f".repeat(64), "0".repeat(32));
    std::fs::write(dir.join(&foreign_name), b"other task name").unwrap();
    std::fs::create_dir(dir.join(format!(
        "a1-{}-{}",
        &kept.id().as_str()[3..67],
        "1".repeat(32)
    )))
    .unwrap();
    let report = store.reconcile(&records("owned")).unwrap();
    assert!(!report.is_consistent());
    assert_eq!(
        report,
        ArtifactReconciliation {
            verified: vec![kept.id().clone()],
            missing: vec![removed.id().clone()],
            mismatched: vec![tampered.id().clone()],
            unrecorded: vec![orphan.id().clone()],
            unexpected_entries: 3,
        }
    );
    assert!(dir.join("notes.txt").exists() && dir.join(orphan.id().as_str()).exists());

    // Records must belong to this store's task and be unique.
    let mut foreign = recorded[0].clone();
    foreign.context = run("other");
    assert_eq!(
        store.reconcile(&[foreign]).err(),
        Some(StoreError::OwnershipMismatch)
    );
    assert_eq!(
        store
            .reconcile(&[recorded[0].clone(), recorded[0].clone()])
            .err(),
        Some(StoreError::InvalidInput)
    );
    journal.close().unwrap();
}

#[test]
fn inventory_limits_fail_closed_instead_of_truncating() {
    let home = private_tempdir();
    let state = StateConfig::resolve(Some(home.path()), None)
        .unwrap()
        .open()
        .unwrap();
    let journal = started(&state, "owned");
    let context = run("owned");
    let lease = state.acquire_task(&TaskId::from("owned")).unwrap();
    let store = ArtifactStore::open(&lease).unwrap();
    let staged = store.stage(ArtifactPersistence::Public, b"x").unwrap();
    // Seed a durable step so raw fixture rows satisfy their run foreign key.
    journal
        .client()
        .commit_step(commit(
            prepare(&journal, &context),
            vec![staged.record(context.clone())],
        ))
        .unwrap()
        .blocking_recv()
        .unwrap();
    journal.close().unwrap();
    let key = &staged.id().as_str()[3..67];
    let id = |n: usize| format!("a1-{key}-{n:032x}");
    let sql =
        rusqlite::Connection::open(state.journal_directory().join("journal.sqlite3")).unwrap();
    // One committed record plus fixtures up to exactly the inventory limit.
    for n in 1..MAX_ARTIFACT_INVENTORY {
        let json = format!(
            r#"{{"id":"{}","context":{{"task_id":"owned","run_id":"run","contract_revision":1}},"sha256":"{}","bytes":1}}"#,
            id(n),
            "0".repeat(64)
        );
        sql.execute(
            "INSERT INTO artifacts VALUES (?1,'owned','run',?2,'now')",
            rusqlite::params![id(n), json],
        )
        .unwrap();
    }
    let load = || {
        let journal = Journal::start(state.journal_directory(), 8).unwrap();
        let result = journal
            .client()
            .load_artifacts(TaskId::from("owned"))
            .unwrap()
            .blocking_recv();
        journal.close().unwrap();
        result
    };
    let all = load().unwrap();
    assert_eq!(all.len(), MAX_ARTIFACT_INVENTORY);
    let over = MAX_ARTIFACT_INVENTORY;
    sql.execute(
        "INSERT INTO artifacts VALUES (?1,'owned','run',?2,'now')",
        rusqlite::params![
            id(over),
            format!(
                r#"{{"id":"{}","context":{{"task_id":"owned","run_id":"run","contract_revision":1}},"sha256":"{}","bytes":1}}"#,
                id(over),
                "0".repeat(64)
            )
        ],
    )
    .unwrap();
    assert_eq!(load().err(), Some(StoreError::BudgetExceeded));
    let mut too_many = all.clone();
    too_many.push(all[0].clone());
    assert_eq!(
        store.reconcile(&too_many).err(),
        Some(StoreError::BudgetExceeded)
    );

    // Directory listings are bounded too.
    let dir = lease.directory().join("artifacts");
    for n in 0..MAX_RECONCILE_ENTRIES {
        std::fs::write(dir.join(format!("extra-{n}")), b"").unwrap();
    }
    assert_eq!(
        store.reconcile(&all[..1]).err(),
        Some(StoreError::BudgetExceeded)
    );
}
