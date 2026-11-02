use serde::Serialize;
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const VERSION_TIMEOUT: Duration = Duration::from_secs(2);
const OUTPUT_LIMIT: u64 = 4096;
const INTEGRITY_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_EXECUTABLE_BYTES: u64 = 1 << 30;

struct ProbeProcess {
    child: Child,
    group: Option<libc::pid_t>,
}

impl ProbeProcess {
    // Internal callers must spawn with process_group(0).
    fn new(child: Child) -> Self {
        let group = libc::pid_t::try_from(child.id())
            .ok()
            .filter(|pid| *pid > 0);
        Self { child, group }
    }

    fn has_exited(&mut self) -> io::Result<bool> {
        // SAFETY: libc's zero-valid C struct has the target ABI/layout. waitid
        // writes it for this owned child only; WNOWAIT does not release its PID.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                self.child.id(),
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ECHILD) {
                // Ownership was lost unexpectedly. Fail closed: do not signal
                // a numeric PID/PGID that another actor may already have reaped.
                self.group = None;
            }
            if error.kind() == io::ErrorKind::Interrupted {
                return Ok(false);
            }
            return Err(error);
        }
        Ok(info.si_signo == libc::SIGCHLD)
    }

    fn stop_group(&mut self) {
        if let Some(group) = self.group.take() {
            // SAFETY: positive private process_group(0) leader, kept unreaped by
            // WNOWAIT until this signal. Taking the ID forbids a post-reap signal.
            unsafe {
                libc::killpg(group, libc::SIGKILL);
            }
        }
    }

    fn finish(mut self) -> io::Result<ExitStatus> {
        self.stop_group();
        self.child.wait()
    }
}

impl Drop for ProbeProcess {
    fn drop(&mut self) {
        if self.group.is_some() {
            self.stop_group();
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[derive(Serialize)]
pub struct ExecutableCheck {
    available: bool,
    path: Option<PathBuf>,
    version: Option<String>,
    error: Option<&'static str>,
}

impl ExecutableCheck {
    pub fn description(&self) -> &str {
        self.version
            .as_deref()
            .or(self.error)
            .unwrap_or("not checked")
    }
}

#[derive(Serialize)]
pub struct Check {
    pub status: &'static str,
    pub detail: &'static str,
}

#[derive(Serialize)]
pub struct Report {
    schema_version: u32,
    pub browser: ExecutableCheck,
    pub bubblewrap: ExecutableCheck,
    pub browser_lock: Check,
    user_namespaces: Check,
    pub sandbox: Check,
    display: Check,
    keyring: Check,
    pkg_config: ExecutableCheck,
    pub runtime_ready: bool,
}

pub fn inspect(chromium: Option<PathBuf>) -> Report {
    let browser = probe(resolve(
        chromium.or_else(|| env::var_os("AERO_CHROMIUM").map(PathBuf::from)),
        &["chromium", "chromium-browser", "google-chrome"],
    ));
    let bubblewrap = probe(resolve(
        env::var_os("AERO_BWRAP").map(PathBuf::from),
        &["bwrap"],
    ));
    let browser_lock = check_browser_pin(&browser);
    let display_configured = ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()));
    Report {
        schema_version: 1,
        browser,
        bubblewrap,
        browser_lock,
        user_namespaces: Check {
            status: "not_checked",
            detail: "Kernel namespace configuration alone does not prove a usable browser sandbox. Required C04 integration is not implemented yet.",
        },
        sandbox: Check {
            status: "not_checked",
            detail: "Browser version and local hash are not sandbox proof. C04 must verify Bubblewrap plus Chromium renderer isolation and process cleanup.",
        },
        display: Check {
            status: if display_configured {
                "configured"
            } else {
                "not_configured"
            },
            detail: "Environment presence only; native X11/Wayland, input and accessibility have not been validated.",
        },
        keyring: Check {
            status: "not_checked",
            detail: "No user credentials accessed. Dedicated Secret Service integration belongs to the vault gate.",
        },
        pkg_config: probe(resolve(None, &["pkg-config"])),
        runtime_ready: false,
    }
}

fn resolve(explicit: Option<PathBuf>, names: &[&str]) -> Option<PathBuf> {
    match explicit {
        Some(path) if path.as_os_str().is_empty() => None,
        Some(path) => executable(&path),
        None => env::var_os("PATH").and_then(|paths| {
            names.iter().find_map(|name| {
                env::split_paths(&paths)
                    .filter(|directory| !directory.as_os_str().is_empty())
                    .find_map(|directory| executable(&directory.join(name)))
            })
        }),
    }
}

fn executable(path: &Path) -> Option<PathBuf> {
    let path = fs::canonicalize(path).ok()?;
    let metadata = path.metadata().ok()?;
    (metadata.is_file() && metadata.permissions().mode() & 0o111 != 0).then_some(path)
}

fn probe(path: Option<PathBuf>) -> ExecutableCheck {
    let result = path
        .as_deref()
        .ok_or("not_found_or_not_executable")
        .and_then(version);
    match result {
        Ok(version) => ExecutableCheck {
            available: true,
            path,
            version: Some(version),
            error: None,
        },
        Err(error) => ExecutableCheck {
            available: false,
            path,
            version: None,
            error: Some(error),
        },
    }
}

// Only trusted local executables are probed. Own/kill this diagnostic's process
// group and reap its direct child, not a browser runtime or hostile-exec sandbox.
fn version(path: &Path) -> Result<String, &'static str> {
    // Refuse inherited auto-reaping/custom handlers before spawning: exclusive
    // ownership must retain the leader's PID until group cleanup is complete.
    if !default_child_wait_policy() {
        return Err("version_wait_policy_unavailable");
    }
    let mut process = ProbeProcess::new(
        Command::new(path)
            .arg("--version")
            .process_group(0)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "version_spawn_failed")?,
    );
    let stdout = process
        .child
        .stdout
        .take()
        .ok_or("version_output_unavailable")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut output = Vec::new();
        let result = stdout.take(OUTPUT_LIMIT + 1).read_to_end(&mut output);
        let _ = sender.send(result.map(|_| output));
    });
    let deadline = Instant::now() + VERSION_TIMEOUT;
    loop {
        match process.has_exited() {
            Ok(true) => break,
            Ok(false) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(false) => {
                return Err("version_timeout");
            }
            Err(_) => {
                return Err("version_wait_failed");
            }
        }
    }
    // The root may exit while a descendant holds stdout open. Clean the group
    // before waiting for EOF, including on normal and nonzero root exits.
    let status = process.finish().map_err(|_| "version_wait_failed")?;
    let output = receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| "version_output_timeout")?
        .map_err(|_| "version_output_failed")?;
    if output.len() as u64 > OUTPUT_LIMIT {
        return Err("version_output_too_large");
    }
    if !status.success() {
        return Err("version_command_failed");
    }
    let text = String::from_utf8(output).map_err(|_| "version_output_invalid")?;
    let text = text.trim();
    if text.is_empty() || text.lines().count() != 1 || text.chars().any(char::is_control) {
        return Err("version_output_invalid");
    }
    Ok(text.to_owned())
}

fn default_child_wait_policy() -> bool {
    // SAFETY: query (never change) SIGCHLD with a valid writable libc C struct.
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    let result = unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) };
    result == 0 && wait_policy_preserves_children(&action)
}

fn wait_policy_preserves_children(action: &libc::sigaction) -> bool {
    action.sa_sigaction == libc::SIG_DFL && action.sa_flags & libc::SA_NOCLDWAIT == 0
}

fn check_browser_pin(browser: &ExecutableCheck) -> Check {
    let Some(path) = browser.path.as_deref().filter(|_| browser.available) else {
        return Check {
            status: "unavailable",
            detail: "No available configured browser to compare with the development pin.",
        };
    };
    let Ok(pin) = serde_json::from_str::<serde_json::Value>(include_str!(
        "../../../packaging/browser-lock.json"
    )) else {
        return Check {
            status: "invalid",
            detail: "Embedded browser pin is invalid JSON.",
        };
    };
    let expected_version = pin.get("version").and_then(|value| value.as_str());
    let expected_hash = pin
        .get("executable_sha256")
        .and_then(|value| value.as_str());
    if pin.get("schema_version").and_then(|value| value.as_u64()) != Some(1)
        || expected_version.is_none()
        || !expected_hash.is_some_and(|hash| {
            hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return Check {
            status: "invalid",
            detail: "Embedded browser pin has an unsupported schema or missing version/hash.",
        };
    }
    if browser
        .version
        .as_deref()
        .and_then(|value| value.split_whitespace().last())
        != expected_version
    {
        return Check {
            status: "mismatch",
            detail: "Browser version differs from the development pin; provision or review the intended version explicitly.",
        };
    }
    match sha256(path) {
        Ok(hash) if Some(hash.as_str()) == expected_hash => Check {
            status: "matched_local_pin",
            detail: "Version and executable bytes match recorded local pins, not an independent vendor signature or sandbox verification. Redistribution is not approved.",
        },
        Ok(_) => Check {
            status: "mismatch",
            detail: "Executable bytes differ from the recorded development pin.",
        },
        Err(error) => Check {
            status: "unavailable",
            detail: match error.kind() {
                io::ErrorKind::TimedOut => {
                    "Executable integrity check exceeded its two-second deadline."
                }
                io::ErrorKind::InvalidData => {
                    "Integrity checking requires a regular executable no larger than 1 GiB."
                }
                _ => "Cannot read the configured browser executable for integrity checking.",
            },
        },
    }
}

fn sha256(path: &Path) -> io::Result<String> {
    let path = path.to_owned();
    integrity_job(INTEGRITY_TIMEOUT, move |deadline| {
        hash_file(&path, deadline)
    })
}

fn integrity_job(
    timeout: Duration,
    job: impl FnOnce(Instant) -> io::Result<String> + Send + 'static,
) -> io::Result<String> {
    let deadline = Instant::now() + timeout;
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = sender.send(job(deadline));
    });
    // Do not join potentially blocked filesystem I/O. Doctor's process exit ends
    // outstanding threads; normal reads also check the deadline between chunks.
    receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "integrity check did not finish"))?
}

fn hash_file(path: &Path, deadline: Instant) -> io::Result<String> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_EXECUTABLE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "executable size/type limit",
        ));
    }
    // A file can grow after metadata was read. Bound the actual bytes too.
    let mut file = file.take(MAX_EXECUTABLE_BYTES + 1);
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 16384];
    let mut total = 0;
    loop {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "integrity deadline",
            ));
        }
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_EXECUTABLE_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "executable size limit",
            ));
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_rejects_auto_reaping_wait_policies() {
        // SAFETY: zero-valid C struct; this test never mutates OS signal policy.
        let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
        action.sa_sigaction = libc::SIG_DFL;
        assert!(wait_policy_preserves_children(&action));
        action.sa_flags = libc::SA_NOCLDWAIT;
        assert!(!wait_policy_preserves_children(&action));
        action.sa_flags = 0;
        action.sa_sigaction = libc::SIG_IGN;
        assert!(!wait_policy_preserves_children(&action));
    }

    #[test]
    fn probe_keeps_leader_unreaped_until_group_cleanup() {
        let child = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .process_group(0)
            .spawn()
            .unwrap();
        let mut process = ProbeProcess::new(child);
        let pid = process.child.id() as libc::pid_t;
        let deadline = Instant::now() + Duration::from_secs(1);
        while !process.has_exited().unwrap() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        // SAFETY: query only the synthetic child's positive PID, no signal.
        let group = unsafe { libc::getpgid(pid) };
        if group < 0 {
            // Contain the known red state: the leader is already reaped, so
            // never send a group signal using that now-unreserved numeric ID.
            process.group = None;
        }
        assert_eq!(group, pid, "leader was reaped before owned-group cleanup");
        process.stop_group();
        assert!(
            process.group.is_none(),
            "cleanup must not signal this group twice"
        );
        assert!(process.finish().unwrap().success());
    }

    #[test]
    fn integrity_deadline_does_not_join_blocked_work() {
        let (release, blocked) = mpsc::channel();
        let (finished, done) = mpsc::channel();
        let start = Instant::now();
        let result = integrity_job(Duration::from_millis(20), move |_| {
            blocked.recv().unwrap();
            finished.send(()).unwrap();
            Ok(String::new())
        });
        release.send(()).unwrap();
        done.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn integrity_hashes_small_files_without_changing_the_digest() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("synthetic-browser");
        let bytes = b"synthetic browser bytes";
        fs::write(&path, bytes).unwrap();
        assert_eq!(
            sha256(&path).unwrap(),
            format!("{:x}", Sha256::digest(bytes))
        );
    }

    #[test]
    fn integrity_rejects_oversized_files_before_hashing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("oversized-synthetic-browser");
        // Sparse: no allocation of a large browser payload or disk data.
        File::create(&path).unwrap().set_len((1 << 30) + 1).unwrap();
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let _ = sender.send(sha256(&path));
        });
        let result = receiver.recv_timeout(Duration::from_secs(1));
        assert!(
            matches!(result, Ok(Err(ref error)) if error.kind() == io::ErrorKind::InvalidData),
            "oversized executable was not rejected before hashing"
        );
    }
}
