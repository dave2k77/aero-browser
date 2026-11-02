//! Private, bounded task-owned byte staging. This is not artifact export,
//! evidence issuance, a journal record, or a crash-durable publication protocol.
//!
//! Only a live host task lease can open this store. Its retained owner guard keeps
//! the cooperative lock alive. As with config, a malicious same-UID/root process
//! is outside the threat model. Failed creates/writes are never cleaned up here.

use crate::config::{TaskLease, TaskOwner, path_key};
use crate::store::StoreError;
use crate::store::worker::ArtifactRecord;
use aero_core::checkpoint::RunContext;
use aero_core::ids::{ArtifactId, TaskId};
use sha2::{Digest, Sha256};
use std::ffi::CString;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::MetadataExt;
use std::sync::Arc;

/// Maximum actual bytes accepted or returned by a single staging operation.
pub const MAX_ARTIFACT_BYTES: usize = 16 * 1024 * 1024;

/// Trusted host assertion, not an automatic classifier or redactor. The caller
/// must establish that the actual bytes contain no secret and are suitable for
/// unencrypted persistence; model/page-provided labels are not authority.
/// Unknown and sensitive material is refused because encrypted staging is absent.
#[derive(Clone, Copy)]
pub enum ArtifactPersistence {
    Public,
    Redacted,
    Unknown,
    Sensitive,
}

/// An in-process receipt for successfully written and independently read bytes.
/// It is not journal metadata, an authority token, or a durable manifest.
#[derive(Clone, Debug)]
pub struct StagedArtifact {
    id: ArtifactId,
    length: usize,
    sha256: String,
}
impl StagedArtifact {
    pub fn id(&self) -> &ArtifactId {
        &self.id
    }
    pub fn len(&self) -> usize {
        self.length
    }
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }
    pub fn sha256(&self) -> &str {
        &self.sha256
    }
    /// Journal attachment describing these read-back bytes for one run.
    pub fn record(&self, context: RunContext) -> ArtifactRecord {
        ArtifactRecord {
            id: self.id.clone(),
            context,
            sha256: self.sha256.clone(),
            bytes: self.length as u64,
        }
    }
}

/// Upper bound on directory entries examined by one reconciliation.
pub const MAX_RECONCILE_ENTRIES: usize = 4096;

/// Snapshot comparison of a task's journal artifact records with its private
/// artifact directory. Each list is sorted by ID. This is a report only:
/// nothing is deleted, repaired or re-recorded (N16 owns cleanup).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ArtifactReconciliation {
    /// Recorded, present, and the actual bytes match the record.
    pub verified: Vec<ArtifactId>,
    /// Recorded, but no file exists.
    pub missing: Vec<ArtifactId>,
    /// Recorded, but the entry is unsafe or its bytes differ from the record.
    pub mismatched: Vec<ArtifactId>,
    /// A regular file with a name this task's store generates, but no record:
    /// typically staged before a crash, failed or uncertain step.
    pub unrecorded: Vec<ArtifactId>,
    /// Any other entry: foreign or malformed names, or non-regular files.
    pub unexpected_entries: usize,
}
impl ArtifactReconciliation {
    pub fn is_consistent(&self) -> bool {
        self.missing.is_empty()
            && self.mismatched.is_empty()
            && self.unrecorded.is_empty()
            && self.unexpected_entries == 0
    }
}

pub struct ArtifactStore {
    directory: File,
    task_key: String,
    // Generated-name namespace: "a1-" artifacts, "e1-" evidence captures.
    prefix: &'static str,
    // Drop the directory before the lock-owning guard.
    _owner: Arc<TaskOwner>,
}
impl ArtifactStore {
    pub fn open(lease: &TaskLease) -> Result<Self, StoreError> {
        Self::open_namespace(lease, "artifacts", ARTIFACT_PREFIX)
    }

    /// The same private store in another task-owned subdirectory and name
    /// namespace (used for evidence captures).
    pub(crate) fn open_namespace(
        lease: &TaskLease,
        subdirectory: &'static str,
        prefix: &'static str,
    ) -> Result<Self, StoreError> {
        let (directory, task_key, owner) = lease.task_subdirectory(subdirectory)?;
        Ok(Self {
            directory,
            task_key,
            prefix,
            _owner: owner,
        })
    }

    /// Creates a fresh generated task-scoped ID; never opens or overwrites an
    /// existing entry. A failed operation can leave an unreachable partial file.
    pub fn stage(
        &self,
        policy: ArtifactPersistence,
        bytes: &[u8],
    ) -> Result<StagedArtifact, StoreError> {
        let id = self.new_id(policy)?;
        self.write_new(id, bytes)
    }

    /// Like [`Self::stage`], but the content is built from the generated name
    /// (an evidence capture embeds its own ID).
    pub(crate) fn stage_with(
        &self,
        policy: ArtifactPersistence,
        build: impl FnOnce(&str) -> Result<Vec<u8>, StoreError>,
    ) -> Result<StagedArtifact, StoreError> {
        let id = self.new_id(policy)?;
        let bytes = build(&id)?;
        self.write_new(id, &bytes)
    }

    fn new_id(&self, policy: ArtifactPersistence) -> Result<String, StoreError> {
        if matches!(
            policy,
            ArtifactPersistence::Unknown | ArtifactPersistence::Sensitive
        ) {
            return Err(StoreError::EncryptionRequired);
        }
        let mut random = [0u8; 16];
        fill_random(&mut random)?;
        let nonce: String = random.iter().map(|b| format!("{b:02x}")).collect();
        Ok(format!("{}{}-{nonce}", self.prefix, self.task_key))
    }

    fn write_new(&self, id: String, bytes: &[u8]) -> Result<StagedArtifact, StoreError> {
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(StoreError::BudgetExceeded);
        }
        let mut file = self.create_exclusive(&id)?;
        file.write_all(bytes).map_err(|_| StoreError::Io)?;
        file.sync_all().map_err(|_| StoreError::Io)?;
        drop(file);
        let receipt = StagedArtifact {
            id: ArtifactId::from(id),
            length: bytes.len(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        };
        // Reopen by name rather than trusting the writer or caller metadata.
        if self.readback(&receipt)? != bytes {
            return Err(StoreError::Conflict);
        }
        Ok(receipt)
    }

    /// Read back the bytes a durable journal record describes. The record must
    /// belong to this store's task, and the actual file must match its recorded
    /// length and SHA-256. This proves byte identity with the record, not that
    /// the bytes are correct, relevant, classified or exportable.
    pub fn verify(&self, record: &ArtifactRecord) -> Result<Vec<u8>, StoreError> {
        if path_key(b"task", record.context.task_id.as_str())? != self.task_key {
            return Err(StoreError::OwnershipMismatch);
        }
        let length = usize::try_from(record.bytes).map_err(|_| StoreError::BudgetExceeded)?;
        self.read_checked(record.id.as_str(), length, &record.sha256)
    }

    /// Compare `records` (from `JournalClient::load_artifacts`) with the files
    /// in this task's artifact directory, reading back every recorded file.
    ///
    /// The result is a snapshot. Run it while this store is not staging and no
    /// step is prepared (e.g. before resuming); a concurrent `stage` can appear
    /// as unrecorded. Records must belong to this task and be unique.
    pub fn reconcile(
        &self,
        records: &[ArtifactRecord],
    ) -> Result<ArtifactReconciliation, StoreError> {
        if records.len() > crate::store::worker::MAX_ARTIFACT_INVENTORY {
            return Err(StoreError::BudgetExceeded);
        }
        let mut recorded = std::collections::HashSet::new();
        for record in records {
            if path_key(b"task", record.context.task_id.as_str())? != self.task_key
                || blob_task_key(self.prefix, record.id.as_str()).ok_or(StoreError::InvalidInput)?
                    != self.task_key
            {
                return Err(StoreError::OwnershipMismatch);
            }
            if !recorded.insert(record.id.as_str()) {
                return Err(StoreError::InvalidInput);
            }
        }
        let mut report = ArtifactReconciliation::default();
        // The live owned descriptor, not a caller path; see store::open.
        let listing = std::fs::read_dir(format!("/proc/self/fd/{}", self.directory.as_raw_fd()))
            .map_err(|_| StoreError::Io)?;
        for (count, entry) in listing.enumerate() {
            if count >= MAX_RECONCILE_ENTRIES {
                return Err(StoreError::BudgetExceeded);
            }
            let entry = entry.map_err(|_| StoreError::Io)?;
            let name = entry.file_name();
            let name = name.to_str();
            if name.is_some_and(|n| recorded.contains(n)) {
                continue;
            }
            // file_type does not follow symlinks.
            let regular = entry.file_type().map_err(|_| StoreError::Io)?.is_file();
            match name {
                Some(n)
                    if regular && blob_task_key(self.prefix, n) == Some(self.task_key.as_str()) =>
                {
                    report.unrecorded.push(ArtifactId::from(n))
                }
                _ => report.unexpected_entries += 1,
            }
        }
        for record in records {
            match self.verify(record) {
                Ok(_) => report.verified.push(record.id.clone()),
                Err(StoreError::NotFound) => report.missing.push(record.id.clone()),
                Err(StoreError::Conflict | StoreError::UnsafePath | StoreError::BudgetExceeded) => {
                    report.mismatched.push(record.id.clone())
                }
                Err(other) => return Err(other),
            }
        }
        for list in [
            &mut report.verified,
            &mut report.missing,
            &mut report.mismatched,
            &mut report.unrecorded,
        ] {
            list.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        }
        Ok(report)
    }

    /// Reopen without following links, validate the private file, bound the read,
    /// and compare the actual length and SHA-256 with the staging receipt.
    pub fn readback(&self, receipt: &StagedArtifact) -> Result<Vec<u8>, StoreError> {
        self.read_checked(receipt.id.as_str(), receipt.length, &receipt.sha256)
    }

    pub(crate) fn read_checked(
        &self,
        id: &str,
        length: usize,
        sha256: &str,
    ) -> Result<Vec<u8>, StoreError> {
        let name = self.checked_name(id)?;
        if length > MAX_ARTIFACT_BYTES {
            return Err(StoreError::BudgetExceeded);
        }
        // SAFETY: valid single component, owned directory fd, no-follow and
        // nonblocking prevent symlink traversal and FIFO hangs.
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            return Err(match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::ENOENT) => StoreError::NotFound,
                _ => StoreError::UnsafePath,
            });
        }
        // SAFETY: fd came from openat and ownership is transferred once.
        let file = unsafe { File::from_raw_fd(fd) };
        let metadata = validate_file(&file)?;
        if metadata.len() > MAX_ARTIFACT_BYTES as u64 {
            return Err(StoreError::BudgetExceeded);
        }
        if metadata.len() != length as u64 {
            return Err(StoreError::Conflict);
        }
        let mut bytes = Vec::with_capacity(length);
        file.take((MAX_ARTIFACT_BYTES as u64) + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| StoreError::Io)?;
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(StoreError::BudgetExceeded);
        }
        if bytes.len() != length || format!("{:x}", Sha256::digest(&bytes)) != sha256 {
            return Err(StoreError::Conflict);
        }
        Ok(bytes)
    }

    fn checked_name(&self, id: &str) -> Result<CString, StoreError> {
        // No arbitrary filenames, separators, NULs, normalization, or cross-task IDs.
        let prefix = blob_task_key(self.prefix, id).ok_or(StoreError::UnsafePath)?;
        if prefix != self.task_key {
            return Err(StoreError::OwnershipMismatch);
        }
        CString::new(id).map_err(|_| StoreError::UnsafePath)
    }

    fn create_exclusive(&self, id: &str) -> Result<File, StoreError> {
        let name = self.checked_name(id)?;
        // SAFETY: validated component and live owned directory fd. O_EXCL means
        // no existing file, symlink, or other entry can be reopened or replaced.
        let fd = unsafe {
            libc::openat(
                self.directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_WRONLY
                    | libc::O_CREAT
                    | libc::O_EXCL
                    | libc::O_NOFOLLOW
                    | libc::O_CLOEXEC
                    | libc::O_NONBLOCK,
                0o600,
            )
        };
        if fd < 0 {
            return Err(match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::EEXIST) => StoreError::Conflict,
                _ => StoreError::Io,
            });
        }
        // SAFETY: ownership of the newly created fd transfers once to File.
        let file = unsafe { File::from_raw_fd(fd) };
        // Correct for a restrictive umask; never change an existing inode.
        if unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
            return Err(StoreError::Io);
        }
        validate_file(&file)?;
        Ok(file)
    }
}

pub(crate) const ARTIFACT_PREFIX: &str = "a1-";
pub(crate) const EVIDENCE_PREFIX: &str = "e1-";

/// The task key embedded in a well-formed generated name in one namespace
/// (`<prefix><64 hex task key>-<32 hex nonce>`), or None for any other string.
fn blob_task_key<'a>(namespace: &str, id: &'a str) -> Option<&'a str> {
    let (prefix, nonce) = id.strip_prefix(namespace)?.split_once('-')?;
    let hex = |v: &str| {
        v.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    };
    (prefix.len() == 64 && nonce.len() == 32 && hex(prefix) && hex(nonce)).then_some(prefix)
}

/// Journal attachment check: an artifact ID must be a generated store name
/// owned by the attaching task. Malformed IDs are invalid input; another
/// task's well-formed ID is an ownership mismatch. Existence and byte identity
/// are established separately by [`ArtifactStore::verify`].
pub(crate) fn check_artifact_owner(task: &TaskId, id: &str) -> Result<(), StoreError> {
    check_owner(ARTIFACT_PREFIX, task, id)
}

/// The evidence-capture equivalent of [`check_artifact_owner`].
pub(crate) fn check_evidence_owner(task: &TaskId, id: &str) -> Result<(), StoreError> {
    check_owner(EVIDENCE_PREFIX, task, id)
}

fn check_owner(namespace: &str, task: &TaskId, id: &str) -> Result<(), StoreError> {
    let prefix = blob_task_key(namespace, id).ok_or(StoreError::InvalidInput)?;
    if prefix != path_key(b"task", task.as_str())? {
        return Err(StoreError::OwnershipMismatch);
    }
    Ok(())
}

fn validate_file(file: &File) -> Result<std::fs::Metadata, StoreError> {
    let metadata = file.metadata().map_err(|_| StoreError::Io)?;
    // SAFETY: geteuid has no preconditions.
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != 0o600
    {
        return Err(StoreError::UnsafePath);
    }
    Ok(metadata)
}

fn fill_random(bytes: &mut [u8]) -> Result<(), StoreError> {
    let mut offset = 0;
    while offset < bytes.len() {
        // SAFETY: writable remaining slice; kernel writes at most its length.
        let n = unsafe {
            libc::getrandom(bytes[offset..].as_mut_ptr().cast(), bytes.len() - offset, 0)
        };
        if n < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(StoreError::Io);
        }
        if n == 0 {
            return Err(StoreError::Io);
        }
        offset += n as usize;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::StateConfig;
    use aero_core::ids::TaskId;
    use std::os::unix::fs::{PermissionsExt, symlink};

    #[test]
    fn exclusive_create_refuses_files_symlinks_and_invalid_names() {
        let home = tempfile::tempdir().unwrap();
        std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let state = StateConfig::resolve(Some(home.path()), None)
            .unwrap()
            .open()
            .unwrap();
        let lease = state.acquire_task(&TaskId::from("task")).unwrap();
        let store = ArtifactStore::open(&lease).unwrap();
        let id = format!("a1-{}-{}", store.task_key, "0".repeat(32));
        let path = lease.directory().join("artifacts").join(&id);
        std::fs::write(&path, b"preserve").unwrap();
        assert!(matches!(
            store.create_exclusive(&id),
            Err(StoreError::Conflict)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), b"preserve");
        std::fs::remove_file(&path).unwrap();
        let target = home.path().join("target");
        std::fs::write(&target, b"safe").unwrap();
        symlink(&target, &path).unwrap();
        assert!(matches!(
            store.create_exclusive(&id),
            Err(StoreError::Conflict)
        ));
        assert_eq!(std::fs::read(target).unwrap(), b"safe");
        for invalid in [
            "../escape",
            "/tmp/x",
            "",
            ".",
            "..",
            "a1-../x",
            "a1-foo\0bar",
        ] {
            assert!(matches!(
                store.create_exclusive(invalid),
                Err(StoreError::UnsafePath)
            ));
        }
        let other = format!("a1-{}-{}", "f".repeat(64), "0".repeat(32));
        assert!(matches!(
            store.create_exclusive(&other),
            Err(StoreError::OwnershipMismatch)
        ));
    }
}
