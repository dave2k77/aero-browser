#![cfg(target_os = "linux")]

use aero_agent::{
    artifacts::ArtifactStore,
    config::{PrivateState, StateConfig},
    retention::{Blocked, preflight},
    store::StoreError,
};
use aero_core::ids::{RunId, RuntimeEpoch, TaskId};
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::Path,
    sync::{Mutex, MutexGuard},
};

// A child spawned by one test can briefly inherit another test's flock fd
// between fork and close-on-exec, making an unrelated post-drop probe Busy.
// Serialize this test binary without weakening any cross-process lock check.
static TEST_ISOLATION: Mutex<()> = Mutex::new(());
fn isolate() -> MutexGuard<'static, ()> {
    TEST_ISOLATION
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

fn fixture() -> (tempfile::TempDir, PrivateState) {
    let home = tempfile::tempdir().unwrap();
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let state = StateConfig::resolve(Some(home.path()), None)
        .unwrap()
        .open()
        .unwrap();
    (home, state)
}
fn id(value: &str) -> TaskId {
    TaskId::from(value)
}
fn entries(path: &Path) -> Vec<std::ffi::OsString> {
    let mut values: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|v| v.unwrap().file_name())
        .collect();
    values.sort();
    values
}

#[test]
fn exclusion_is_retained_and_independent_scopes_preserve_sentinels() {
    let _isolation = isolate();
    let (home, state) = fixture();
    let a = state.acquire_task(&id("a/../b")).unwrap();
    let b = state.acquire_task(&id("b")).unwrap();
    let a_path = a.directory().to_owned();
    let b_path = b.directory().to_owned();
    assert_ne!(a_path, b_path);
    fs::write(a_path.join("sentinel"), b"a").unwrap();
    fs::write(b_path.join("sentinel"), b"b").unwrap();
    fs::write(home.path().join("outside"), b"outside").unwrap();
    assert!(matches!(
        preflight(&state, &id("a/../b")),
        Err(StoreError::Busy)
    ));
    drop(a);
    let guard = preflight(&state, &id("a/../b")).unwrap();
    assert_eq!(
        guard.blocked(),
        Blocked {
            retention_hold_unknown: true,
            journal_pending_unknown: true
        }
    );
    assert!(matches!(
        state.acquire_task(&id("a/../b")),
        Err(StoreError::Busy)
    ));
    assert!(matches!(preflight(&state, &id("b")), Err(StoreError::Busy)));
    drop(b);
    let b_guard = preflight(&state, &id("b")).unwrap();
    drop(b_guard);
    let lock_before = fs::metadata(a_path.join("controller.lock")).unwrap().ino();
    drop(guard);
    assert_eq!(
        fs::metadata(a_path.join("controller.lock")).unwrap().ino(),
        lock_before
    );
    assert!(state.acquire_task(&id("a/../b")).is_ok());
    assert_eq!(fs::read(a_path.join("sentinel")).unwrap(), b"a");
    assert_eq!(fs::read(b_path.join("sentinel")).unwrap(), b"b");
    assert_eq!(fs::read(home.path().join("outside")).unwrap(), b"outside");
}

#[test]
fn derived_handles_keep_task_busy() {
    let _isolation = isolate();
    let (_home, state) = fixture();
    let task = state.acquire_task(&id("task")).unwrap();
    let artifacts = ArtifactStore::open(&task).unwrap();
    let runtime = task
        .create_runtime(&RunId::from("run"), &RuntimeEpoch::from("epoch"))
        .unwrap();
    drop(task);
    assert!(matches!(
        preflight(&state, &id("task")),
        Err(StoreError::Busy)
    ));
    drop(artifacts);
    assert!(matches!(
        preflight(&state, &id("task")),
        Err(StoreError::Busy)
    ));
    drop(runtime);
    assert!(preflight(&state, &id("task")).is_ok());

    let task = state.acquire_task(&id("other")).unwrap();
    let artifacts = ArtifactStore::open(&task).unwrap();
    let runtime = task
        .create_runtime(&RunId::from("run"), &RuntimeEpoch::from("epoch"))
        .unwrap();
    drop(task);
    drop(runtime);
    assert!(matches!(
        preflight(&state, &id("other")),
        Err(StoreError::Busy)
    ));
    drop(artifacts);
    assert!(preflight(&state, &id("other")).is_ok());
}

#[test]
fn missing_and_unsafe_paths_never_create_or_follow() {
    let _isolation = isolate();
    let (home, state) = fixture();
    let tasks = state.root().join("tasks");
    let before = entries(&tasks);
    assert!(matches!(
        preflight(&state, &id("missing")),
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        preflight(&state, &id("")),
        Err(StoreError::InvalidInput)
    ));
    assert!(matches!(
        preflight(&state, &id("\0")),
        Err(StoreError::InvalidInput)
    ));
    assert_eq!(entries(&tasks), before);
    let lease = state.acquire_task(&id("one")).unwrap();
    let path = lease.directory().to_owned();
    drop(lease);
    let lock = path.join("controller.lock");
    fs::remove_file(&lock).unwrap();
    let before = entries(&path);
    assert!(matches!(
        preflight(&state, &id("one")),
        Err(StoreError::NotFound)
    ));
    assert_eq!(entries(&path), before);
    let outside = home.path().join("outside");
    fs::write(&outside, b"sentinel").unwrap();
    symlink(&outside, &lock).unwrap();
    assert!(matches!(
        preflight(&state, &id("one")),
        Err(StoreError::UnsafePath)
    ));
    assert_eq!(fs::read(&outside).unwrap(), b"sentinel");
    fs::remove_file(&lock).unwrap();
    fs::remove_dir(&path).unwrap();
    symlink(home.path(), &path).unwrap();
    assert!(matches!(
        preflight(&state, &id("one")),
        Err(StoreError::UnsafePath)
    ));
    assert_eq!(fs::read(&outside).unwrap(), b"sentinel");
}

#[test]
fn unsafe_modes_hardlinks_and_replaced_tasks_scope_are_rejected() {
    let _isolation = isolate();
    let (_home, state) = fixture();
    let lease = state.acquire_task(&id("one")).unwrap();
    let path = lease.directory().to_owned();
    drop(lease);
    let lock = path.join("controller.lock");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        preflight(&state, &id("one")),
        Err(StoreError::UnsafePath)
    ));
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(matches!(
        preflight(&state, &id("one")),
        Err(StoreError::UnsafePath)
    ));
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(&lock, path.join("hardlink")).unwrap();
    assert!(matches!(
        preflight(&state, &id("one")),
        Err(StoreError::UnsafePath)
    ));
    fs::remove_file(path.join("hardlink")).unwrap();
    let tasks = state.root().join("tasks");
    fs::rename(&tasks, state.root().join("detached-tasks")).unwrap();
    fs::create_dir(&tasks).unwrap();
    fs::set_permissions(&tasks, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(
        preflight(&state, &id("one")),
        Err(StoreError::UnsafePath)
    ));
}

#[test]
fn foreign_private_state_and_uncreated_scopes_are_not_authority() {
    let _isolation = isolate();
    let (_home_a, a) = fixture();
    let (_home_b, b) = fixture();
    let lease = a.acquire_task(&id("same")).unwrap();
    let sentinel = lease.directory().join("sentinel");
    fs::write(&sentinel, b"owned by a").unwrap();
    drop(lease);
    assert!(matches!(
        preflight(&b, &id("same")),
        Err(StoreError::NotFound)
    ));
    let guard = preflight(&a, &id("same")).unwrap();
    assert!(guard.blocked().journal_pending_unknown && guard.blocked().retention_hold_unknown);
    assert_eq!(fs::read(sentinel).unwrap(), b"owned by a");
}

#[test]
fn child_process_lock_probe() {
    let _isolation = isolate();
    let Some(home) = std::env::var_os("AERO_RETENTION_CHILD_HOME") else {
        return;
    };
    let state = StateConfig::resolve(Some(Path::new(&home)), None)
        .unwrap()
        .open()
        .unwrap();
    if std::env::var_os("AERO_RETENTION_CHILD_MODE").is_some() {
        assert!(matches!(
            state.acquire_task(&id("cross-process")),
            Err(StoreError::Busy)
        ));
    } else {
        assert!(matches!(
            preflight(&state, &id("cross-process")),
            Err(StoreError::Busy)
        ));
    }
}

#[test]
fn task_lease_and_preflight_exclude_each_other_across_processes() {
    let _isolation = isolate();
    let (home, state) = fixture();
    let lease = state.acquire_task(&id("cross-process")).unwrap();
    let child = |mode: Option<&str>| {
        let mut command = std::process::Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", "child_process_lock_probe", "--nocapture"])
            .env("AERO_RETENTION_CHILD_HOME", home.path())
            .env_remove("AERO_RETENTION_CHILD_MODE");
        if let Some(mode) = mode {
            command.env("AERO_RETENTION_CHILD_MODE", mode);
        }
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    };
    child(None);
    drop(lease);
    let guard = preflight(&state, &id("cross-process")).unwrap();
    child(Some("lease"));
    drop(guard);
}

#[test]
fn fresh_lock_excludes_another_process() {
    let _isolation = isolate();
    let (home, state) = fixture();
    let lease = state.acquire_task(&id("cross-process")).unwrap();
    drop(lease);
    let guard = preflight(&state, &id("cross-process")).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "child_process_lock_probe", "--nocapture"])
        .env("AERO_RETENTION_CHILD_HOME", home.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
    drop(guard);
    assert!(preflight(&state, &id("cross-process")).is_ok());
}
