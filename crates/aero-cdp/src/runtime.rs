//! C04: own one sandboxed Chromium process from spawn to verified teardown.
//!
//! [`BrowserProcess::launch`] spawns a [`SandboxSpec`] with an empty
//! environment, drains stderr into a bounded redacted ring, locates the
//! sandbox's PID-namespace init (Bubblewrap's direct child) and holds a pidfd
//! on it, then waits for a fresh, strictly formatted `DevToolsActivePort`.
//!
//! Teardown SIGKILLs the namespace init through its pidfd, then kills and
//! reaps Bubblewrap. The kernel completes the init's exit only after every
//! process in its PID namespace (including nested namespaces Chromium creates)
//! has exited, so the pidfd becoming readable proves the sandbox is empty.
//! Bubblewrap's `--die-with-parent` alone is not relied on: the init arms it
//! after starting, so a kill during startup would leave the sandbox running.
//! Every readiness failure takes this path before returning.
//!
//! The profile directory is writable from inside the sandbox, so the port file
//! is read without following symlinks, without blocking on FIFOs, and with a
//! size limit.

use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use aero_core::ids::{RuntimeEpoch, TargetId};

use crate::evaluate::EvaluateError;
use crate::frame_manager::FrameManager;
use crate::launcher::{SandboxSpec, Viewport};
use crate::page::Page;
use crate::target_registry::{AttachError, TargetInfo, TargetRegistry};
use crate::{CdpConnection, CdpError, TransportLimits};

/// How long Bubblewrap may take to create its namespace init. Separate from
/// the readiness deadline so teardown can always be verified.
const SANDBOX_START_TIMEOUT: Duration = Duration::from_secs(2);
/// Largest accepted `DevToolsActivePort`.
const MAX_PORT_FILE_BYTES: u64 = 4096;

/// Bounds for one launch.
#[derive(Clone, Debug)]
pub struct LaunchLimits {
    /// From spawn until the DevTools endpoint is known.
    pub readiness_timeout: Duration,
    /// Bytes of browser stderr retained (most recent).
    pub stderr_bytes: usize,
    /// How long teardown waits for the sandbox to empty after the kill.
    pub teardown_timeout: Duration,
}

impl Default for LaunchLimits {
    fn default() -> Self {
        Self {
            readiness_timeout: Duration::from_secs(15),
            stderr_bytes: 64 * 1024,
            teardown_timeout: Duration::from_secs(5),
        }
    }
}

/// Why a launch failed. The sandbox was torn down before this was returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchError {
    /// The profile already holds a `DevToolsActivePort`; it is not a fresh
    /// run directory. Nothing was spawned and nothing was deleted.
    StaleProfile,
    /// Bubblewrap could not be spawned.
    Spawn(std::io::ErrorKind),
    /// Bubblewrap did not start its sandbox namespace.
    SandboxNotStarted { stderr: String },
    /// The sandbox exited before the browser was ready.
    EarlyExit {
        code: Option<i32>,
        signal: Option<i32>,
        stderr: String,
    },
    /// No valid DevTools endpoint appeared before the deadline.
    ReadinessTimeout { stderr: String },
    /// The sandbox could not be verified empty after teardown.
    TeardownIncomplete,
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StaleProfile => f.write_str("the browser profile is not fresh"),
            Self::Spawn(kind) => write!(f, "could not spawn the sandbox: {kind}"),
            Self::SandboxNotStarted { .. } => f.write_str("the sandbox namespace did not start"),
            Self::EarlyExit { code, signal, .. } => {
                write!(
                    f,
                    "the sandbox exited early (code {code:?}, signal {signal:?})"
                )
            }
            Self::ReadinessTimeout { .. } => {
                f.write_str("the browser did not become ready in time")
            }
            Self::TeardownIncomplete => f.write_str("the sandbox could not be verified empty"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// Outcome of a teardown.
#[derive(Debug)]
pub struct ShutdownReport {
    /// Bubblewrap's exit status (normally killed by SIGKILL).
    pub status: Option<ExitStatus>,
    /// True once the sandbox PID namespace was observed empty.
    pub sandbox_emptied: bool,
}

/// Why a browser runtime operation failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeError {
    Launch(LaunchError),
    Cdp(CdpError),
    Attach(AttachError),
    /// The page's session is no longer owned by the registry.
    Detached,
    /// The browser rejected a navigation (its `errorText`, untrusted).
    NavigationFailed(String),
    /// No tab appeared before the deadline.
    NoTab,
    /// A page evaluation needed by the operation failed.
    Evaluate(EvaluateError),
    /// The new document's HTTP status was 4xx or 5xx.
    HttpStatus(u16),
    /// The new document did not reach DOMContentLoaded before the deadline.
    NavigationTimeout,
    /// The frame is not (or no longer) in the page's frame graph.
    FrameGone,
    /// The frame graph is being rebuilt after missed events.
    FramesUnknown,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Launch(error) => write!(f, "browser launch failed: {error}"),
            Self::Cdp(error) => write!(f, "CDP error: {error}"),
            Self::Attach(error) => write!(f, "attach failed: {error:?}"),
            Self::Detached => f.write_str("the page session is detached"),
            Self::NavigationFailed(_) => f.write_str("the browser rejected the navigation"),
            Self::NoTab => f.write_str("no tab appeared in time"),
            Self::Evaluate(error) => write!(f, "page evaluation failed: {error}"),
            Self::HttpStatus(status) => write!(f, "the document returned HTTP {status}"),
            Self::NavigationTimeout => f.write_str("the navigation did not finish in time"),
            Self::FrameGone => f.write_str("the frame is gone"),
            Self::FramesUnknown => f.write_str("the frame graph is being rebuilt"),
        }
    }
}

impl std::error::Error for RuntimeError {}

impl From<CdpError> for RuntimeError {
    fn from(error: CdpError) -> Self {
        Self::Cdp(error)
    }
}

/// The owned browser: sandboxed process, its CDP connection and the target
/// registry. There is no global connection; everything browser-side is
/// reached through this value. Field order is drop order.
pub struct BrowserRuntime {
    registry: TargetRegistry,
    connection: Arc<CdpConnection>,
    process: Option<BrowserProcess>,
    epoch: RuntimeEpoch,
    viewport: Viewport,
}

impl BrowserRuntime {
    /// Launch, connect, and start target discovery. `epoch` identifies this
    /// runtime generation (the B06 runtime directory's epoch); observations
    /// and handles are bound to it.
    pub async fn start(
        spec: &SandboxSpec,
        limits: LaunchLimits,
        epoch: RuntimeEpoch,
    ) -> Result<Self, RuntimeError> {
        // On any later error the launched process is dropped, which tears the
        // sandbox down.
        let process = BrowserProcess::launch(spec, limits)
            .await
            .map_err(RuntimeError::Launch)?;
        let connection =
            Arc::new(CdpConnection::connect(process.endpoint(), TransportLimits::default()).await?);
        let registry = TargetRegistry::start(Arc::clone(&connection)).await?;
        Ok(Self {
            registry,
            connection,
            process: Some(process),
            epoch,
            viewport: spec.viewport(),
        })
    }

    /// This runtime generation.
    pub fn epoch(&self) -> &RuntimeEpoch {
        &self.epoch
    }

    /// The browser-level connection (no session). Page methods sent here fail:
    /// use a [`Page`].
    pub fn connection(&self) -> &CdpConnection {
        &self.connection
    }

    pub fn targets(&self) -> Vec<TargetInfo> {
        self.registry.targets()
    }

    pub fn tabs(&self) -> Vec<TargetInfo> {
        self.registry.tabs()
    }

    pub fn target(&self, id: &TargetId) -> Option<TargetInfo> {
        self.registry.target(id)
    }

    /// Why the target registry stopped following events, if it has.
    pub fn registry_failure(&self) -> Option<String> {
        self.registry.failure()
    }

    /// Times the target registry rebuilt itself after missing events.
    pub fn resyncs(&self) -> u64 {
        self.registry.resyncs()
    }

    /// The first tab, waiting up to `timeout` for one to be discovered.
    pub async fn wait_for_tab(&self, timeout: Duration) -> Result<TargetInfo, RuntimeError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(tab) = self.tabs().into_iter().next() {
                return Ok(tab);
            }
            if Instant::now() >= deadline {
                return Err(RuntimeError::NoTab);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    /// Attach to a tab; the registry refuses a second session for it.
    pub async fn attach(&self, target: &TargetId) -> Result<Page, RuntimeError> {
        let session = self
            .registry
            .attach(target)
            .await
            .map_err(RuntimeError::Attach)?;
        let frames = FrameManager::start(Arc::clone(self.registry.connection()), session.clone())
            .await
            .map_err(RuntimeError::Cdp)?;
        let page = Page::new(
            Arc::clone(self.registry.connection()),
            self.registry.handle(),
            target.clone(),
            session,
            frames,
        );
        // `--window-size` sizes the window, not the page: apply the root
        // viewport (implementation plan §4: CSS px at device scale factor 1).
        page.call(
            "Emulation.setDeviceMetricsOverride",
            serde_json::json!({
                "width": self.viewport.width,
                "height": self.viewport.height,
                "deviceScaleFactor": 1,
                "mobile": false,
            }),
        )
        .await?;
        Ok(page)
    }

    /// Stop the registry, close the connection and tear the sandbox down.
    pub async fn shutdown(mut self) -> ShutdownReport {
        self.registry.stop().await;
        self.connection.close().await;
        match self.process.take() {
            Some(process) => tokio::task::spawn_blocking(move || process.shutdown())
                .await
                .unwrap_or(ShutdownReport {
                    status: None,
                    sandbox_emptied: false,
                }),
            None => ShutdownReport {
                status: None,
                sandbox_emptied: false,
            },
        }
    }
}

/// One running sandboxed browser. Dropping it tears the sandbox down
/// (blocking for at most the teardown timeout).
pub struct BrowserProcess {
    child: Option<Child>,
    /// Bubblewrap's PID-namespace init, once observed.
    init: Option<NamespaceInit>,
    endpoint: String,
    stderr: Arc<Mutex<Ring>>,
    reader: Option<StderrReader>,
    limits: LaunchLimits,
}

struct NamespaceInit {
    pidfd: OwnedFd,
}

struct StderrReader {
    thread: JoinHandle<()>,
    done: mpsc::Receiver<()>,
}

impl BrowserProcess {
    /// Spawn the sandbox and wait for the DevTools endpoint.
    pub async fn launch(spec: &SandboxSpec, limits: LaunchLimits) -> Result<Self, LaunchError> {
        let port_file = spec.devtools_active_port();
        if std::fs::symlink_metadata(port_file).is_ok() {
            return Err(LaunchError::StaleProfile);
        }
        let started = Instant::now();
        let mut child = Command::new(spec.program())
            .args(spec.args())
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .map_err(|error| LaunchError::Spawn(error.kind()))?;
        let stderr = Arc::new(Mutex::new(Ring::new(limits.stderr_bytes)));
        let reader = child
            .stderr
            .take()
            .map(|pipe| StderrReader::spawn(pipe, Arc::clone(&stderr)));
        let bwrap = child.id();
        let mut process = Self {
            child: Some(child),
            init: None,
            endpoint: String::new(),
            stderr,
            reader,
            limits,
        };

        // Every return below this point tears the sandbox down first.
        let start_deadline = started + SANDBOX_START_TIMEOUT;
        while process.init.is_none() {
            if let Some(status) = process.exited() {
                return Err(process.fail(|stderr| early_exit(status, stderr)));
            }
            process.init = find_namespace_init(bwrap);
            if process.init.is_none() {
                if Instant::now() >= start_deadline {
                    return Err(process.fail(|stderr| LaunchError::SandboxNotStarted { stderr }));
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }

        let deadline = started + process.limits.readiness_timeout;
        loop {
            if let Some(status) = process.exited() {
                return Err(process.fail(|stderr| early_exit(status, stderr)));
            }
            let endpoint = read_port_file(port_file).and_then(|bytes| parse_port_file(&bytes));
            if let Some((port, path)) = endpoint {
                // The writer must still be the live sandbox we own.
                if process.exited().is_none() {
                    process.endpoint = format!("ws://127.0.0.1:{port}{path}");
                    return Ok(process);
                }
                continue;
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(process.fail(|stderr| LaunchError::ReadinessTimeout { stderr }));
            }
            tokio::time::sleep((deadline - now).min(Duration::from_millis(25))).await;
        }
    }

    /// `ws://127.0.0.1:PORT/devtools/browser/UUID`.
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Process ID of Bubblewrap (the direct child), for diagnostics.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(Child::id)
    }

    /// The retained, redacted end of the browser's stderr.
    pub fn stderr_tail(&self) -> String {
        lock(&self.stderr).text()
    }

    /// Kill the sandbox, reap Bubblewrap and wait (bounded) until the
    /// sandbox namespace is empty. Blocking; use `spawn_blocking` from async.
    pub fn shutdown(mut self) -> ShutdownReport {
        self.terminate()
    }

    /// Bubblewrap's exit status if it has exited (it is then reaped).
    fn exited(&mut self) -> Option<ExitStatus> {
        self.child.as_mut()?.try_wait().ok().flatten()
    }

    /// Tear down, then build the launch error from the full stderr. An
    /// observed sandbox that could not be verified empty overrides the cause.
    fn fail(&mut self, error: impl FnOnce(String) -> LaunchError) -> LaunchError {
        let report = self.terminate();
        if self.init.is_some() && !report.sandbox_emptied {
            return LaunchError::TeardownIncomplete;
        }
        error(self.stderr_tail())
    }

    fn terminate(&mut self) -> ShutdownReport {
        // Kill the namespace init through its pidfd first: the kernel then
        // kills every process in the namespace. Killing only Bubblewrap is not
        // enough, because --die-with-parent is armed by the init itself after
        // it starts; a kill during that window leaves the sandbox running.
        if let Some(init) = &self.init {
            pidfd_kill(&init.pidfd);
        }
        let mut status = None;
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            status = child.wait().ok();
        }
        let sandbox_emptied = self
            .init
            .as_ref()
            .is_some_and(|init| wait_readable(&init.pidfd, self.limits.teardown_timeout));
        if let Some(reader) = self.reader.take() {
            // The pipe reaches EOF once no sandbox process holds it.
            let wait = if sandbox_emptied || self.init.is_none() {
                Duration::from_secs(1)
            } else {
                Duration::ZERO
            };
            reader.join(wait);
        }
        ShutdownReport {
            status,
            sandbox_emptied,
        }
    }
}

impl Drop for BrowserProcess {
    fn drop(&mut self) {
        if self.child.is_some() {
            self.terminate();
        }
    }
}

fn early_exit(status: ExitStatus, stderr: String) -> LaunchError {
    LaunchError::EarlyExit {
        code: status.code(),
        signal: status.signal(),
        stderr,
    }
}

impl StderrReader {
    fn spawn(mut pipe: std::process::ChildStderr, ring: Arc<Mutex<Ring>>) -> Self {
        let (finished, done) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut chunk = [0u8; 4096];
            while let Ok(read) = pipe.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                lock(&ring).push(&chunk[..read]);
            }
            let _ = finished.send(());
        });
        Self { thread, done }
    }

    /// Join if the reader finishes within `wait`; otherwise leave it detached
    /// (it ends when the last holder of the pipe exits).
    fn join(self, wait: Duration) {
        if self.done.recv_timeout(wait).is_ok() {
            let _ = self.thread.join();
        }
    }
}

/// Find Bubblewrap's direct child, provided it runs in a PID namespace other
/// than ours, and pin it with a pidfd.
fn find_namespace_init(bwrap: u32) -> Option<NamespaceInit> {
    let own_namespace = std::fs::read_link("/proc/self/ns/pid").ok()?;
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if parent_of(pid) != Some(bwrap) {
            continue;
        }
        let pidfd = pidfd_open(pid)?;
        // Recheck after pinning: still Bubblewrap's child, in a new namespace.
        let namespace = std::fs::read_link(format!("/proc/{pid}/ns/pid")).ok()?;
        if parent_of(pid) == Some(bwrap) && namespace != own_namespace {
            return Some(NamespaceInit { pidfd });
        }
    }
    None
}

/// Parent PID from `/proc/<pid>/stat` (the command name may contain `)`).
fn parent_of(pid: u32) -> Option<u32> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let (_, after_name) = stat.rsplit_once(')')?;
    after_name.split_whitespace().nth(1)?.parse().ok()
}

fn pidfd_open(pid: u32) -> Option<OwnedFd> {
    let pid = libc::pid_t::try_from(pid).ok()?;
    // SAFETY: pidfd_open takes a PID and flags and returns a new descriptor
    // or -1; no memory is passed.
    let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    let fd = i32::try_from(fd).ok().filter(|fd| *fd >= 0)?;
    // SAFETY: the kernel just returned this descriptor and nothing else owns it.
    Some(unsafe { OwnedFd::from_raw_fd(fd) })
}

/// SIGKILL the process a pidfd refers to (never a reused PID).
fn pidfd_kill(pidfd: &OwnedFd) {
    // SAFETY: pidfd_send_signal takes a descriptor, a signal number, a null
    // siginfo and flags; no memory is passed.
    unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            pidfd.as_raw_fd(),
            libc::SIGKILL,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        );
    }
}

/// Wait until `fd` is readable (a pidfd: the process has fully exited).
fn wait_readable(fd: &OwnedFd, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let millis = i32::try_from(remaining.as_millis()).unwrap_or(i32::MAX);
        let mut poll = libc::pollfd {
            fd: fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd for the duration of the call.
        let ready = unsafe { libc::poll(&mut poll, 1, millis) };
        if ready > 0 {
            return poll.revents & libc::POLLIN != 0;
        }
        let interrupted =
            ready < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted;
        if !interrupted || remaining.is_zero() {
            return false;
        }
    }
}

/// The most recent `capacity` bytes written.
#[derive(Default)]
struct Ring {
    capacity: usize,
    bytes: VecDeque<u8>,
}

impl Ring {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            bytes: VecDeque::new(),
        }
    }
    fn push(&mut self, chunk: &[u8]) {
        let keep = chunk.len().min(self.capacity);
        self.bytes.extend(&chunk[chunk.len() - keep..]);
        let excess = self.bytes.len().saturating_sub(self.capacity);
        self.bytes.drain(..excess);
    }
    fn text(&self) -> String {
        let bytes: Vec<u8> = self.bytes.iter().copied().collect();
        redact(&String::from_utf8_lossy(&bytes))
    }
}

/// Redact browser stderr for diagnostics: URL queries and fragments are
/// removed and control characters other than newline and tab are replaced.
pub(crate) fn redact(text: &str) -> String {
    fn flush(token: &mut String, out: &mut String) {
        if let Some(scheme) = token.find("://")
            && let Some(cut) = token[scheme..].find(['?', '#'])
        {
            token.truncate(scheme + cut);
            token.push_str("?[redacted]");
        }
        out.push_str(token);
        token.clear();
    }
    let mut out = String::with_capacity(text.len());
    let mut token = String::new();
    for c in text.chars() {
        let c = if c.is_control() && c != '\n' && c != '\t' {
            '\u{fffd}'
        } else {
            c
        };
        if c.is_whitespace() {
            flush(&mut token, &mut out);
            out.push(c);
        } else {
            token.push(c);
        }
    }
    flush(&mut token, &mut out);
    out
}

/// Parse `DevToolsActivePort`: a nonzero port line and a
/// `/devtools/browser/<lowercase UUID>` line, optionally newline-terminated.
pub(crate) fn parse_port_file(bytes: &[u8]) -> Option<(u16, String)> {
    let text = std::str::from_utf8(bytes).ok()?;
    let text = text.strip_suffix('\n').unwrap_or(text);
    let (port, path) = text.split_once('\n')?;
    if !(1..=5).contains(&port.len()) || !port.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let port = port.parse::<u16>().ok().filter(|port| *port != 0)?;
    let id = path.strip_prefix("/devtools/browser/")?;
    let uuid = id.len() == 36
        && id.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        });
    uuid.then(|| (port, path.to_owned()))
}

/// Read a small regular file without following a final symlink or blocking on
/// a FIFO. `None` for anything else, a missing file, or more than 4096 bytes.
pub(crate) fn read_port_file(path: &Path) -> Option<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_PORT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_PORT_FILE_BYTES).then_some(bytes)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    const UUID: &str = "4f63093e-6535-41a9-9332-3ea6340dcc7f";

    #[test]
    fn port_file_parsing_is_strict() {
        let ok = format!("42011\n/devtools/browser/{UUID}");
        assert_eq!(
            parse_port_file(ok.as_bytes()),
            Some((42011, format!("/devtools/browser/{UUID}")))
        );
        assert_eq!(
            parse_port_file(format!("{ok}\n").as_bytes()).map(|p| p.0),
            Some(42011)
        );
        for bad in [
            String::new(),
            "42011".into(),
            "42011\n".into(),
            format!("0\n/devtools/browser/{UUID}"),
            format!("65536\n/devtools/browser/{UUID}"),
            format!("+42011\n/devtools/browser/{UUID}"),
            format!(" 42011\n/devtools/browser/{UUID}"),
            format!("42011\n/devtools/page/{UUID}"),
            format!("42011\n/devtools/browser/{}", &UUID[..20]),
            format!("42011\n/devtools/browser/{}", UUID.to_uppercase()),
            format!("42011\n/devtools/browser/{UUID}?x=1"),
            format!("42011\n/devtools/browser/{UUID}\nextra"),
            format!("42011\r\n/devtools/browser/{UUID}"),
        ] {
            assert_eq!(parse_port_file(bad.as_bytes()), None, "{bad:?}");
        }
    }

    #[test]
    fn port_file_reads_refuse_symlinks_fifos_and_large_files() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::write(&real, b"42011\n").unwrap();
        assert_eq!(read_port_file(&real), Some(b"42011\n".to_vec()));
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert_eq!(read_port_file(&link), None);
        let fifo = dir.path().join("fifo");
        let c_path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        assert_eq!(
            read_port_file(&fifo),
            None,
            "must not block or accept a FIFO"
        );
        let large = dir.path().join("large");
        std::fs::write(&large, vec![b'1'; 4097]).unwrap();
        assert_eq!(read_port_file(&large), None);
        assert_eq!(read_port_file(&dir.path().join("missing")), None);
        assert_eq!(read_port_file(dir.path()), None);
    }

    #[test]
    fn stderr_is_bounded_and_redacted() {
        let mut ring = Ring {
            capacity: 8,
            ..Ring::default()
        };
        ring.push(b"0123456789abc");
        assert_eq!(ring.text(), "56789abc");
        ring.push(b"XY");
        assert_eq!(
            ring.text(),
            "789abcXY",
            "older bytes are dropped across pushes"
        );
        assert_eq!(
            redact("GET https://x.test/a?token=s3cret#frag done\x1b[0m\tok\n"),
            "GET https://x.test/a?[redacted] done\u{fffd}[0m\tok\n"
        );
        assert_eq!(
            redact("ws://127.0.0.1:1/devtools"),
            "ws://127.0.0.1:1/devtools"
        );
    }

    fn process(child: Child, init: OwnedFd) -> BrowserProcess {
        BrowserProcess {
            child: Some(child),
            init: Some(NamespaceInit { pidfd: init }),
            endpoint: String::new(),
            stderr: Arc::new(Mutex::new(Ring::new(16))),
            reader: None,
            limits: LaunchLimits {
                teardown_timeout: Duration::from_millis(50),
                ..LaunchLimits::default()
            },
        }
    }

    #[test]
    fn teardown_reports_empty_only_after_the_init_exits() {
        // An init that exits when killed: verified empty.
        let mut init = Command::new("sleep").arg("30").spawn().unwrap();
        let pidfd = pidfd_open(init.id()).unwrap();
        let bwrap = Command::new("sleep").arg("30").spawn().unwrap();
        let report = process(bwrap, pidfd).terminate();
        assert!(report.sandbox_emptied);
        assert_eq!(report.status.and_then(|s| s.signal()), Some(libc::SIGKILL));
        assert_eq!(
            init.wait().unwrap().signal(),
            Some(libc::SIGKILL),
            "killed via pidfd"
        );

        // An init that outlives the timeout: never claimed empty. PID 1
        // cannot be signalled by an unprivileged user (EPERM), and as root the
        // test is skipped rather than signalling it at all.
        // SAFETY: geteuid has no preconditions.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }
        let bwrap = Command::new("sleep").arg("30").spawn().unwrap();
        let report = process(bwrap, pidfd_open(1).unwrap()).terminate();
        assert!(!report.sandbox_emptied);
    }
}
