//! Task retention: a read-only preflight and the file half of coordinated
//! deletion.
//!
//! `preflight` never creates or removes task content or the stable lock inode;
//! it only excludes cooperative TaskLease, ArtifactStore and RuntimeDirs owners
//! while its guard is alive and always reports blocked. Deletion itself is
//! `TaskAuthority::delete_task`: journal tombstone and record removal first,
//! then the crate-private `purge_task_files` under the task lock. Copied paths, untracked
//! writers, and malicious same-UID/root processes are outside these guarantees.

use crate::config::{PrivateState, TaskLease, path_key};
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

/// Upper bounds for one purge; exceeding either leaves the deletion pending.
const MAX_PURGE_ENTRIES: usize = 100_000;
const MAX_PURGE_DEPTH: usize = 32;

/// Remove everything inside a locked task directory except its permanent
/// `controller.lock`, then sync the directory. Called only by the deletion
/// coordinator after the journal has recorded the `deleting` tombstone.
///
/// Descriptor-relative and no-follow: symlinks are unlinked, never followed;
/// hard links lose only this name; a subdirectory on another device (a mount)
/// is refused (UnsafePath) rather than crossed. Errors leave the tombstone
/// `deleting` so `TaskAuthority::resume_deletions` can retry. Unlinking is not
/// secure erasure and cannot recall exported or remote copies.
pub(crate) fn purge_task_files(lease: &TaskLease) -> Result<(), StoreError> {
    let shell = lease.shell();
    let device = shell.metadata().map_err(|_| StoreError::Io)?.dev();
    let mut budget = MAX_PURGE_ENTRIES;
    for name in entry_names(shell)? {
        if name.as_bytes() == b"controller.lock" {
            continue;
        }
        remove_entry(shell, &name, device, 0, &mut budget)?;
    }
    shell.sync_all().map_err(|_| StoreError::Io)
}

fn entry_names(directory: &File) -> Result<Vec<CString>, StoreError> {
    use std::os::unix::ffi::OsStrExt;
    // The live owned descriptor, as the store opener and reconciliation do.
    let listing = std::fs::read_dir(format!("/proc/self/fd/{}", directory.as_raw_fd()))
        .map_err(|_| StoreError::Io)?;
    let mut names = Vec::new();
    for entry in listing {
        let entry = entry.map_err(|_| StoreError::Io)?;
        names.push(CString::new(entry.file_name().as_bytes()).map_err(|_| StoreError::UnsafePath)?);
    }
    Ok(names)
}

fn remove_entry(
    parent: &File,
    name: &CString,
    device: u64,
    depth: usize,
    budget: &mut usize,
) -> Result<(), StoreError> {
    *budget = budget.checked_sub(1).ok_or(StoreError::BudgetExceeded)?;
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: valid parent fd, NUL-terminated single component, writable buffer.
    if unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            &mut stat,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    } != 0
    {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::ENOENT) => Ok(()),
            _ => Err(StoreError::Io),
        };
    }
    if stat.st_mode & libc::S_IFMT == libc::S_IFDIR {
        if stat.st_dev != device {
            return Err(StoreError::UnsafePath);
        }
        if depth >= MAX_PURGE_DEPTH {
            return Err(StoreError::BudgetExceeded);
        }
        // SAFETY: as above; O_NOFOLLOW|O_DIRECTORY refuse a swapped-in link.
        let fd = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(StoreError::UnsafePath);
        }
        // SAFETY: new owned descriptor from openat.
        let child = unsafe { File::from_raw_fd(fd) };
        let opened = child.metadata().map_err(|_| StoreError::Io)?;
        if opened.dev() != stat.st_dev || opened.ino() != stat.st_ino {
            return Err(StoreError::UnsafePath);
        }
        for inner in entry_names(&child)? {
            remove_entry(&child, &inner, device, depth + 1, budget)?;
        }
        drop(child);
        // SAFETY: valid parent fd and name; removes only an empty directory.
        if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
            return Err(StoreError::Io);
        }
    } else {
        // SAFETY: valid parent fd and name; unlinks the entry itself, never a
        // symlink target.
        if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0
            && std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOENT)
        {
            return Err(StoreError::Io);
        }
    }
    Ok(())
}
