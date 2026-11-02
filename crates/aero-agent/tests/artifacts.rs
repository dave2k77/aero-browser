#![cfg(target_os = "linux")]

use aero_agent::{
    artifacts::{ArtifactPersistence, ArtifactStore, MAX_ARTIFACT_BYTES},
    config::StateConfig,
    store::StoreError,
};
use aero_core::ids::TaskId;
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
