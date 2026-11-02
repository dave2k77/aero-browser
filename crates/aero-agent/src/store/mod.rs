//! Private SQLite schema/open boundary. The worker module owns B05's bounded
//! writer and domain transactions. This module does not execute tasks, grant authority,
//! encrypt snapshots or turn raw checkpoint/form values into safe metadata.

use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};
use std::time::Duration;

pub mod worker;

const INITIAL_SCHEMA: &str = include_str!("migrations/001_initial.sql");
const APPLICATION_ID: i64 = 0x4145524f;
const DATABASE_NAME: &str = "journal.sqlite3";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StoreError {
    UnsafePath,
    Busy,
    Io,
    Database,
    CorruptDatabase,
    UnsupportedVersion,
    ForeignDatabase,
    SchemaMismatch,
    InvalidInput,
    OwnershipMismatch,
    NotFound,
    Conflict,
    BudgetExceeded,
    NeedsReview,
    EncryptionRequired,
    QueueFull,
    WorkerClosed,
    PermissionDenied,
    StartBlocked,
    UnknownPricing,
    /// The task was deleted; its ID cannot be reused in this state root.
    Deleted,
    /// A retention hold prevents starting deletion.
    Held,
}
impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::UnsafePath => "unsafe journal storage path or permissions",
            Self::Busy => "journal already has an owner",
            Self::Io => "journal filesystem operation failed",
            Self::Database => "journal database operation failed",
            Self::CorruptDatabase => "journal database is corrupt",
            Self::UnsupportedVersion => "unsupported journal schema version",
            Self::ForeignDatabase => "journal belongs to another application",
            Self::SchemaMismatch => "journal schema does not match the supported version",
            Self::InvalidInput => "invalid journal request",
            Self::OwnershipMismatch => "journal ownership mismatch",
            Self::NotFound => "journal record not found",
            Self::Conflict => "journal state conflict",
            Self::BudgetExceeded => "journal resource budget exceeded",
            Self::NeedsReview => "journal has an unresolved effect",
            Self::EncryptionRequired => "encrypted persistence is required but unavailable",
            Self::QueueFull => "journal queue is full; request was not accepted",
            Self::WorkerClosed => "journal worker unavailable; reconcile accepted requests",
            Self::PermissionDenied => "caller does not have task authority",
            Self::StartBlocked => "task start prerequisites are not satisfied",
            Self::UnknownPricing => "inference monetary ceiling requires known upper-bound pricing",
            Self::Deleted => "task was deleted and its ID cannot be reused",
            Self::Held => "a retention hold prevents deleting this task",
        })
    }
}
impl std::error::Error for StoreError {}

#[derive(Debug, PartialEq, Eq)]
pub struct StoreSettings {
    pub schema_version: i64,
    pub foreign_keys: bool,
    pub journal_mode: String,
    pub synchronous: i64,
    pub busy_timeout_ms: i64,
}

/// Synchronous exclusive journal owner, not a writer queue or raw-SQL adapter.
/// This dedicated directory protects against other UIDs, not a malicious owner/root.
pub struct Store {
    // Close SQLite/sidecars before releasing the cooperative owner lock.
    connection: Connection,
    _database_file: File,
    _lock: File,
    _directory: File,
}
impl Store {
    /// Absolute private path; only the last directory component may be created.
    pub fn open(directory: &Path) -> Result<Self, StoreError> {
        let directory = private_directory(directory)?;
        let lock = private_file(&directory, "journal.lock", true)?.ok_or(StoreError::Io)?;
        // SAFETY: live owned fd, cooperative nonblocking exclusive flock.
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(match std::io::Error::last_os_error().raw_os_error() {
                Some(libc::EWOULDBLOCK) => StoreError::Busy,
                _ => StoreError::Io,
            });
        }
        check_sidecars(&directory)?;
        let database_file = private_file(&directory, DATABASE_NAME, true)?.ok_or(StoreError::Io)?;
        // SQLite resolves the owned descriptor path; the name is internal, never a URI.
        let path = format!("/proc/self/fd/{}/{}", directory.as_raw_fd(), DATABASE_NAME);
        let mut connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(database_error)?;
        connection
            .busy_timeout(Duration::from_millis(5000))
            .map_err(database_error)?;
        connection
            .pragma_update(None, "foreign_keys", true)
            .map_err(database_error)?;
        connection
            .pragma_update(None, "trusted_schema", false)
            .map_err(database_error)?;
        connection
            .pragma_update(None, "recursive_triggers", true)
            .map_err(database_error)?;
        initialize_or_validate(&mut connection)?;
        let mode: String = connection
            .pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))
            .map_err(database_error)?;
        if mode != "wal" {
            return Err(StoreError::Database);
        }
        connection
            .pragma_update(None, "synchronous", "FULL")
            .map_err(database_error)?;
        check_sidecars(&directory)?;
        let store = Self {
            connection,
            _database_file: database_file,
            _lock: lock,
            _directory: directory,
        };
        let settings = store.settings()?;
        if !settings.foreign_keys || settings.synchronous != 2 || settings.busy_timeout_ms != 5000 {
            return Err(StoreError::Database);
        }
        Ok(store)
    }
    pub fn settings(&self) -> Result<StoreSettings, StoreError> {
        Ok(StoreSettings {
            schema_version: self
                .connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(|_| StoreError::Database)?,
            foreign_keys: self
                .connection
                .pragma_query_value(None, "foreign_keys", |row| row.get(0))
                .map_err(|_| StoreError::Database)?,
            journal_mode: self
                .connection
                .pragma_query_value(None, "journal_mode", |row| row.get(0))
                .map_err(|_| StoreError::Database)?,
            synchronous: self
                .connection
                .pragma_query_value(None, "synchronous", |row| row.get(0))
                .map_err(|_| StoreError::Database)?,
            busy_timeout_ms: self
                .connection
                .pragma_query_value(None, "busy_timeout", |row| row.get(0))
                .map_err(|_| StoreError::Database)?,
        })
    }
}

fn database_error(error: rusqlite::Error) -> StoreError {
    match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase) => {
            StoreError::CorruptDatabase
        }
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            StoreError::Busy
        }
        _ => StoreError::Database,
    }
}

fn initialize_or_validate(connection: &mut Connection) -> Result<(), StoreError> {
    let version: i64 = connection
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(database_error)?;
    if version != 0 && version != 1 {
        return Err(StoreError::UnsupportedVersion);
    }
    let app: i64 = connection
        .pragma_query_value(None, "application_id", |row| row.get(0))
        .map_err(database_error)?;
    // Validate physical integrity before creating any schema in a version-zero file.
    let integrity: String = connection
        .query_row("PRAGMA integrity_check(1)", [], |row| row.get(0))
        .map_err(database_error)?;
    if integrity != "ok" {
        return Err(StoreError::CorruptDatabase);
    }
    if version == 0 {
        let objects: i64 = connection
            .query_row("SELECT count(*) FROM sqlite_schema", [], |row| row.get(0))
            .map_err(database_error)?;
        if objects != 0 || app == APPLICATION_ID {
            return Err(StoreError::SchemaMismatch);
        }
        if app != 0 {
            return Err(StoreError::ForeignDatabase);
        }
        initialize(connection, INITIAL_SCHEMA)?;
    } else if app != APPLICATION_ID {
        return Err(StoreError::ForeignDatabase);
    }

    let mut foreign = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(database_error)?;
    if foreign.exists([]).map_err(database_error)? {
        return Err(StoreError::CorruptDatabase);
    }
    let expected = Connection::open_in_memory().map_err(database_error)?;
    expected
        .execute_batch(INITIAL_SCHEMA)
        .map_err(database_error)?;
    if schema_identity(connection)? != schema_identity(&expected)? {
        return Err(StoreError::SchemaMismatch);
    }
    Ok(())
}

fn initialize(connection: &mut Connection, sql: &str) -> Result<(), StoreError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(database_error)?;
    transaction.execute_batch(sql).map_err(database_error)?;
    transaction.commit().map_err(database_error)
}

fn schema_identity(
    connection: &Connection,
) -> Result<Vec<(String, String, String, String)>, StoreError> {
    let mut statement = connection.prepare("SELECT type,name,tbl_name,coalesce(sql,'') FROM sqlite_schema WHERE name NOT GLOB 'sqlite_*' ORDER BY type,name").map_err(database_error)?;
    statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .map_err(database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_error)
}

fn private_directory(path: &Path) -> Result<File, StoreError> {
    open_private_directory(path, false)
}

/// Config-only recursive creation. Existing ancestors retain B04 trust checks;
/// existing leaf permissions are never repaired or widened.
pub(crate) fn private_directory_tree(path: &Path) -> Result<File, StoreError> {
    open_private_directory(path, true)
}

fn open_private_directory(path: &Path, create_parents: bool) -> Result<File, StoreError> {
    if !path.is_absolute() {
        return Err(StoreError::UnsafePath);
    }
    let components: Vec<_> = path.components().collect();
    if components.len() < 2
        || components
            .iter()
            .skip(1)
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(StoreError::UnsafePath);
    }
    let mut directory = File::open("/").map_err(|_| StoreError::Io)?;
    for (index, component) in components.iter().enumerate().skip(1) {
        let Component::Normal(name) = component else {
            return Err(StoreError::UnsafePath);
        };
        let name = CString::new(name.as_bytes()).map_err(|_| StoreError::UnsafePath)?;
        let last = index == components.len() - 1;
        let mut created = false;
        if last || create_parents {
            // SAFETY: owned directory fd, NUL-terminated single path component.
            if unsafe { libc::mkdirat(directory.as_raw_fd(), name.as_ptr(), 0o700) } == 0 {
                created = true;
            } else if std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                return Err(StoreError::Io);
            }
        }
        // SAFETY: each component opened separately, without following links.
        let fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(StoreError::UnsafePath);
        }
        // SAFETY: newly returned fd, ownership transferred exactly once.
        directory = unsafe { File::from_raw_fd(fd) };
        // SAFETY: only a newly created owned inode is chmodded via its live fd.
        if created && unsafe { libc::fchmod(directory.as_raw_fd(), 0o700) } != 0 {
            return Err(StoreError::Io);
        }
        let metadata = directory.metadata().map_err(|_| StoreError::Io)?;
        // Ancestors must not let another UID replace the private leaf. A root-
        // owned sticky directory (e.g. /tmp) retains ordinary Unix owner protection.
        let uid = unsafe { libc::geteuid() };
        if (metadata.uid() != uid && metadata.uid() != 0)
            || (!last
                && metadata.mode() & 0o022 != 0
                && !(metadata.uid() == 0 && metadata.mode() & 0o1000 != 0))
        {
            return Err(StoreError::UnsafePath);
        }
        if last {
            let metadata = directory.metadata().map_err(|_| StoreError::Io)?;
            // SAFETY: geteuid takes no pointers; no process state is changed.
            if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o7777 != 0o700 {
                return Err(StoreError::UnsafePath);
            }
        }
    }
    Ok(directory)
}

pub(crate) fn private_child_directory(
    parent: &File,
    name: &str,
    exclusive: bool,
) -> Result<File, StoreError> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') {
        return Err(StoreError::UnsafePath);
    }
    let name = CString::new(name).map_err(|_| StoreError::UnsafePath)?;
    // SAFETY: live validated directory fd and one checked NUL-terminated component.
    let created = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } == 0;
    if !created {
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
            return Err(StoreError::Io);
        }
        if exclusive {
            return Err(StoreError::Conflict);
        }
    }
    // SAFETY: open the child itself, never a symlink, with unique fd ownership.
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
    let directory = unsafe { File::from_raw_fd(fd) };
    if created && unsafe { libc::fchmod(directory.as_raw_fd(), 0o700) } != 0 {
        return Err(StoreError::Io);
    }
    let metadata = directory.metadata().map_err(|_| StoreError::Io)?;
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.mode() & 0o7777 != 0o700 {
        return Err(StoreError::UnsafePath);
    }
    Ok(directory)
}

pub(crate) fn private_file(
    directory: &File,
    name: &str,
    create: bool,
) -> Result<Option<File>, StoreError> {
    let name = CString::new(name).map_err(|_| StoreError::UnsafePath)?;
    let flags = libc::O_RDWR | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
    let mut created = false;
    let mut fd = -1;
    if create {
        // SAFETY: owned directory fd, fixed internal name, exclusive create.
        fd = unsafe {
            libc::openat(
                directory.as_raw_fd(),
                name.as_ptr(),
                flags | libc::O_CREAT | libc::O_EXCL,
                0o600,
            )
        };
        if fd >= 0 {
            created = true;
        } else if std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
            return Err(StoreError::UnsafePath);
        }
    }
    if fd < 0 {
        // SAFETY: same owned directory/name; no create/truncate for old data.
        fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return if !create
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT)
            {
                Ok(None)
            } else {
                Err(StoreError::UnsafePath)
            };
        }
    }
    // SAFETY: newly opened fd, transferred exactly once into File.
    let file = unsafe { File::from_raw_fd(fd) };
    if created && unsafe { libc::fchmod(file.as_raw_fd(), 0o600) } != 0 {
        return Err(StoreError::Io);
    }
    let metadata = file.metadata().map_err(|_| StoreError::Io)?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o7777 != 0o600
    {
        return Err(StoreError::UnsafePath);
    }
    Ok(Some(file))
}

fn check_sidecars(directory: &File) -> Result<(), StoreError> {
    for name in [
        "journal.sqlite3-wal",
        "journal.sqlite3-shm",
        "journal.sqlite3-journal",
    ] {
        let name = CString::new(name).map_err(|_| StoreError::UnsafePath)?;
        // fstatat rather than open/close: closing an extra fd can release the
        // process's POSIX locks on an inode already owned by SQLite.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::fstatat(
                directory.as_raw_fd(),
                name.as_ptr(),
                &mut stat,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        };
        if result != 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
                continue;
            }
            return Err(StoreError::UnsafePath);
        }
        if stat.st_mode & libc::S_IFMT != libc::S_IFREG
            || stat.st_nlink != 1
            || stat.st_uid != unsafe { libc::geteuid() }
            || stat.st_mode & 0o7777 != 0o600
        {
            return Err(StoreError::UnsafePath);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sqlite_created_wal_and_shm_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let parent = tempfile::tempdir().unwrap();
        std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = parent.path().join("store");
        let store = Store::open(&path).unwrap();
        store.connection.execute("INSERT INTO tasks(id,status,created_at,updated_at) VALUES ('synthetic','draft','now','now')",[]).unwrap();
        for name in [
            "journal.sqlite3",
            "journal.sqlite3-wal",
            "journal.sqlite3-shm",
            "journal.lock",
        ] {
            let metadata = std::fs::symlink_metadata(path.join(name)).unwrap();
            assert!(metadata.is_file());
            assert_eq!(metadata.mode() & 0o7777, 0o600);
            assert_eq!(metadata.nlink(), 1);
        }
        check_sidecars(&store._directory).unwrap();
    }

    #[test]
    fn failed_initialization_rolls_back_schema_and_version() {
        let mut connection = Connection::open_in_memory().unwrap();
        let invalid = format!("{INITIAL_SCHEMA}\nCREATE TABLE tasks(id TEXT);");
        assert!(initialize(&mut connection, &invalid).is_err());
        assert!(schema_identity(&connection).unwrap().is_empty());
        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            connection
                .pragma_query_value(None, "application_id", |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
