//! Read-only filesystem preflight, **not** permission or a guarantee to delete.
//!
//! This module never creates or removes task content or the stable lock inode. A
//! successful lock only excludes cooperative TaskLease, ArtifactStore and
//! RuntimeDirs owners while the returned guard is alive. Copied paths, untracked
//! writers, and malicious same-UID/root processes are outside that guarantee.
//! The journal has no unified writer gate/tombstone and no retention-hold store
//! exists. Consequently every acquired guard remains blocked: neither a caller's
//! assertion nor a model-provided status can authorize deletion. Even a future
//! journal read could race a subsequent write (TOCTOU) unless admission and
//! deletion are coordinated by an authoritative protocol. This API does not
//! establish journal task ownership or provide any deletion operation.

use crate::config::{PrivateState, path_key};
use crate::store::{StoreError, private_file};
use aero_core::ids::TaskId;
use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;

/// Filesystem exclusion only. There is deliberately no deletion-eligible state.
/// Drop this guard to release the fresh exclusive task lock.
pub struct PreflightGuard {
    // Drop the directory before the lock; both remain alive through the guard.
    _directory: File,
    _lock: File,
}

/// Missing authoritative checks; neither can be supplied by the caller.
#[derive(Debug, PartialEq, Eq)]
pub struct Blocked {
    pub retention_hold_unknown: bool,
    pub journal_pending_unknown: bool,
}

impl PreflightGuard {
    /// Always blocked until real hold and coordinated journal authorities exist.
    pub fn blocked(&self) -> Blocked {
        Blocked {
            retention_hold_unknown: true,
            journal_pending_unknown: true,
        }
    }
}

/// Check one opaque task ID in this already-open private state, and acquire its
/// existing stable controller lock nonblockingly. This performs no creation;
/// obtaining `state` through `StateConfig::open` can itself create directories.
/// A task ID selects a filesystem scope; it is not proof of journal ownership.
pub fn preflight(state: &PrivateState, task: &TaskId) -> Result<PreflightGuard, StoreError> {
    let key = path_key(b"task", task.as_str())?;
    let (root, tasks) = state.retention_directories();
    check_private_directory(root)?;
    check_private_directory(tasks)?;
    check_link(root, "tasks", tasks)?;

    let name = CString::new(key).map_err(|_| StoreError::UnsafePath)?;
    // SAFETY: descriptor-relative lookup of a single hash component, no create
    // and no symlink traversal. O_DIRECTORY also rejects non-directories.
    let fd = unsafe {
        libc::openat(
            tasks.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ENOENT) => StoreError::NotFound,
            _ => StoreError::UnsafePath,
        });
    }
    // SAFETY: successful openat produced a new owned descriptor.
    let directory = unsafe { File::from_raw_fd(fd) };
    check_private_directory(&directory)?;
    let lock = private_file(&directory, "controller.lock", false)?.ok_or(StoreError::NotFound)?;
    // SAFETY: lock is an open descriptor to the existing private regular file.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err(match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EWOULDBLOCK) => StoreError::Busy,
            _ => StoreError::Io,
        });
    }
    // Confirm the locked inode and task remain linked at the expected names.
    // This is not protection against a malicious concurrent same-UID process.
    check_link(root, "tasks", tasks)?;
    check_link(
        tasks,
        name.to_str().map_err(|_| StoreError::UnsafePath)?,
        &directory,
    )?;
    check_link(&directory, "controller.lock", &lock)?;
    Ok(PreflightGuard {
        _directory: directory,
        _lock: lock,
    })
}

fn check_private_directory(directory: &File) -> Result<(), StoreError> {
    let metadata = directory.metadata().map_err(|_| StoreError::Io)?;
    if !metadata.is_dir()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != 0o700
    {
        return Err(StoreError::UnsafePath);
    }
    Ok(())
}

fn check_link(parent: &File, name: &str, file: &File) -> Result<(), StoreError> {
    let name = CString::new(name).map_err(|_| StoreError::UnsafePath)?;
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: valid parent fd, single NUL-terminated name, writable stat buffer;
    // do not follow symlinks when comparing the directory entry to the fd.
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return Err(StoreError::UnsafePath);
    }
    let metadata = file.metadata().map_err(|_| StoreError::Io)?;
    if stat.st_dev != metadata.dev()
        || stat.st_ino != metadata.ino()
        || (stat.st_mode & libc::S_IFMT) != (metadata.mode() & libc::S_IFMT)
    {
        return Err(StoreError::UnsafePath);
    }
    Ok(())
}
