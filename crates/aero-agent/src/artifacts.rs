//! Private, bounded task-owned byte staging. This is not artifact export,
//! evidence issuance, a journal record, or a crash-durable publication protocol.
//!
//! Only a live host task lease can open this store. Its retained owner guard keeps
//! the cooperative lock alive. As with config, a malicious same-UID/root process
//! is outside the threat model. Failed creates/writes are never cleaned up here.

use crate::config::{TaskLease, TaskOwner};
use crate::store::StoreError;
use aero_core::ids::ArtifactId;
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
}

pub struct ArtifactStore {
    directory: File,
    task_key: String,
    // Drop the directory before the lock-owning guard.
    _owner: Arc<TaskOwner>,
}
impl ArtifactStore {
    pub fn open(lease: &TaskLease) -> Result<Self, StoreError> {
        let (directory, task_key, owner) = lease.artifact_directory()?;
        Ok(Self {
            directory,
            task_key,
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
        if matches!(
            policy,
            ArtifactPersistence::Unknown | ArtifactPersistence::Sensitive
        ) {
            return Err(StoreError::EncryptionRequired);
        }
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(StoreError::BudgetExceeded);
        }
        let mut random = [0u8; 16];
        fill_random(&mut random)?;
        let nonce: String = random.iter().map(|b| format!("{b:02x}")).collect();
        let id = format!("a1-{}-{nonce}", self.task_key);
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

    /// Reopen without following links, validate the private file, bound the read,
    /// and compare the actual length and SHA-256 with the staging receipt.
    pub fn readback(&self, receipt: &StagedArtifact) -> Result<Vec<u8>, StoreError> {
        let name = self.checked_name(receipt.id.as_str())?;
        if receipt.length > MAX_ARTIFACT_BYTES {
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
        if metadata.len() != receipt.length as u64 {
            return Err(StoreError::Conflict);
        }
        let mut bytes = Vec::with_capacity(receipt.length);
        file.take((MAX_ARTIFACT_BYTES as u64) + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| StoreError::Io)?;
        if bytes.len() > MAX_ARTIFACT_BYTES {
            return Err(StoreError::BudgetExceeded);
        }
        if bytes.len() != receipt.length
            || format!("{:x}", Sha256::digest(&bytes)) != receipt.sha256
        {
            return Err(StoreError::Conflict);
        }
        Ok(bytes)
    }

    fn checked_name(&self, id: &str) -> Result<CString, StoreError> {
        // No arbitrary filenames, separators, NULs, normalization, or cross-task IDs.
        let Some((prefix, nonce)) = id.strip_prefix("a1-").and_then(|v| v.split_once('-')) else {
            return Err(StoreError::UnsafePath);
        };
        if prefix.len() != 64
            || nonce.len() != 32
            || !prefix
                .bytes()
                .chain(nonce.bytes())
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(StoreError::UnsafePath);
        }
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
