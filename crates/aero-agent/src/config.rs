//! Linux private state and cooperative task-controller ownership.
//!
//! Paths are not authorization capabilities. N02 owns trusted Task/run issuance;
//! C04/N08 own actual browser/input/takeover lifetime. Keep RuntimeDirs alive until
//! all users of its profile/staging paths have stopped. Dropping leases never
//! deletes state or resets journal budgets. N16 owns future validated cleanup.
//! This protects against other UIDs, not a malicious same-UID/root process.
//! When integrating both owners: acquire the application Journal first, then a
//! task lease, then runtime directories. On failure drop acquired guards; never
//! delete a lock to force progress. Stop/join runtime consumers before releasing
//! their handles. Journal and task locks are independent, not interchangeable.

use crate::store::{StoreError, private_child_directory, private_directory_tree, private_file};
use aero_core::ids::{RunId, RuntimeEpoch, TaskId};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::os::fd::AsRawFd;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

/// Pure configuration. No environment mutation or filesystem writes on resolution.
pub struct StateConfig {
    root: PathBuf,
}
impl StateConfig {
    pub fn from_env() -> Result<Self, StoreError> {
        let home = std::env::var_os("HOME");
        let state = std::env::var_os("XDG_STATE_HOME");
        Self::resolve(
            home.as_deref().map(Path::new),
            state.as_deref().map(Path::new),
        )
    }
    /// Empty XDG_STATE_HOME is unset; an explicit relative value fails closed.
    /// Runtime data is private state, not /tmp or an assumed XDG_RUNTIME_DIR.
    pub fn resolve(home: Option<&Path>, state_home: Option<&Path>) -> Result<Self, StoreError> {
        let base = match state_home.filter(|p| !p.as_os_str().is_empty()) {
            Some(path) => path.to_path_buf(),
            None => home
                .filter(|p| !p.as_os_str().is_empty())
                .ok_or(StoreError::InvalidInput)?
                .join(".local/state"),
        };
        if !base.is_absolute() || base.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err(StoreError::InvalidInput);
        }
        Ok(Self {
            root: base.join("aerobrowser"),
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn open(&self) -> Result<PrivateState, StoreError> {
        let root = private_directory_tree(&self.root)?;
        let journal = private_child_directory(&root, "journal", false)?;
        let tasks = private_child_directory(&root, "tasks", false)?;
        Ok(PrivateState {
            root_path: self.root.clone(),
            journal_path: self.root.join("journal"),
            _root: root,
            _journal: journal,
            tasks,
        })
    }
}

pub struct PrivateState {
    root_path: PathBuf,
    journal_path: PathBuf,
    _root: File,
    _journal: File,
    tasks: File,
}
impl PrivateState {
    pub fn root(&self) -> &Path {
        &self.root_path
    }
    /// The one journal remains separate from task/run/profile directories.
    pub fn journal_directory(&self) -> &Path {
        &self.journal_path
    }
    // Descriptor-only access for the read-only retention preflight; never a path
    // supplied by the caller. The retention module checks these live descriptors.
    pub(crate) fn retention_directories(&self) -> (&File, &File) {
        (&self._root, &self.tasks)
    }
    pub fn acquire_task(&self, id: &TaskId) -> Result<TaskLease, StoreError> {
        self.acquire_task_key(path_key(b"task", id.as_str())?)
    }
    /// Lock a task directory by its hashed key (resuming a deletion only knows
    /// the key). Same creation, ownership and lock rules as `acquire_task`.
    pub(crate) fn acquire_task_key(&self, key: String) -> Result<TaskLease, StoreError> {
        if key.len() != 64
            || !key
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(StoreError::InvalidInput);
        }
        let directory = private_child_directory(&self.tasks, &key, false)?;
        let lock = private_file(&directory, "controller.lock", true)?.ok_or(StoreError::Io)?;
        // SAFETY: live owned fd, cooperative exclusive nonblocking lock. Never
        // unlink this inode: replacement would split owners across two locks.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::EWOULDBLOCK) => StoreError::Busy,
                _ => StoreError::Io,
            });
        }
        Ok(TaskLease {
            owner: Arc::new(TaskOwner {
                path: self.root_path.join("tasks").join(&key),
                key,
                directory,
                _lock: lock,
            }),
        })
    }
}
pub(crate) struct TaskOwner {
    path: PathBuf,
    key: String,
    directory: File,
    _lock: File,
}
/// Non-cloneable controller lease. Derived runtime handles extend its lock lifetime.
pub struct TaskLease {
    owner: Arc<TaskOwner>,
}
impl TaskLease {
    /// Open a private store subdirectory (e.g. "artifacts", "evidence")
    /// relative to the owned task descriptor. The returned owner guard keeps
    /// the task lock alive with the directory.
    pub(crate) fn task_subdirectory(
        &self,
        name: &'static str,
    ) -> Result<(File, String, Arc<TaskOwner>), StoreError> {
        let directory = private_child_directory(&self.owner.directory, name, false)?;
        Ok((directory, self.owner.key.clone(), Arc::clone(&self.owner)))
    }

    pub fn directory(&self) -> &Path {
        &self.owner.path
    }
    /// The locked task directory descriptor, for N16 cleanup in this crate.
    pub(crate) fn shell(&self) -> &File {
        &self.owner.directory
    }
    /// Each launch/rebuild needs a fresh RuntimeEpoch. Existing epochs are never
    /// reopened, overwritten or cleaned up, including partial failed creation.
    pub fn create_runtime(
        &self,
        run: &RunId,
        epoch: &RuntimeEpoch,
    ) -> Result<RuntimeDirs, StoreError> {
        let run_key = path_key(b"run", run.as_str())?;
        let epoch_key = path_key(b"runtime", epoch.as_str())?;
        let runs = private_child_directory(&self.owner.directory, "runs", false)?;
        let run_dir = private_child_directory(&runs, &run_key, false)?;
        let runtimes = private_child_directory(&run_dir, "runtimes", false)?;
        let runtime = private_child_directory(&runtimes, &epoch_key, true)?;
        let profile = private_child_directory(&runtime, "profile", false)?;
        let staging = private_child_directory(&runtime, "staging", false)?;
        let base = self
            .owner
            .path
            .join("runs")
            .join(run_key)
            .join("runtimes")
            .join(epoch_key);
        Ok(RuntimeDirs {
            profile_path: base.join("profile"),
            staging_path: base.join("staging"),
            _profile: profile,
            _staging: staging,
            _runtime: runtime,
            _owner: Arc::clone(&self.owner),
        })
    }
}
/// Keep this handle, not just copied path strings, for the entire runtime lifetime.
/// These empty directories are not a running browser or an artifact writer.
pub struct RuntimeDirs {
    profile_path: PathBuf,
    staging_path: PathBuf,
    _profile: File,
    _staging: File,
    _runtime: File,
    // Released after directory fds; retains task ownership if TaskLease is dropped.
    _owner: Arc<TaskOwner>,
}
impl RuntimeDirs {
    pub fn profile_directory(&self) -> &Path {
        &self.profile_path
    }
    pub fn staging_directory(&self) -> &Path {
        &self.staging_path
    }
}
pub(crate) fn path_key(domain: &[u8], id: &str) -> Result<String, StoreError> {
    if id.trim().is_empty() || id.len() > 256 || id.contains('\0') {
        return Err(StoreError::InvalidInput);
    }
    // Domain-separated fixed-length keys avoid path injection/NAME_MAX. IDs are
    // opaque bytes: no trimming, case folding, URL decoding or path normalization.
    let mut hash = Sha256::new();
    hash.update(b"aerobrowser-state-v1\0");
    hash.update(domain);
    hash.update(b"\0");
    hash.update(id.as_bytes());
    Ok(format!("{:x}", hash.finalize()))
}
