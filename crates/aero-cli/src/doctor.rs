use serde::Serialize;
use sha2::{Digest, Sha256};
use std::env;
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const VERSION_TIMEOUT: Duration = Duration::from_secs(2);
const OUTPUT_LIMIT: u64 = 4096;

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

// Only explicit trusted local executable paths are probed. This is not a browser
// launcher or a sandbox: the later runtime owns browsing and descendant cleanup.
fn version(path: &Path) -> Result<String, &'static str> {
    let mut child = Command::new(path)
        .arg("--version")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "version_spawn_failed")?;
    let stdout = child.stdout.take().ok_or("version_output_unavailable")?;
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let mut output = Vec::new();
        let result = stdout.take(OUTPUT_LIMIT + 1).read_to_end(&mut output);
        let _ = sender.send(result.map(|_| output));
    });
    let deadline = Instant::now() + VERSION_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("version_timeout");
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("version_wait_failed");
            }
        }
    };
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
        Err(_) => Check {
            status: "unavailable",
            detail: "Cannot read the configured browser executable for integrity checking.",
        },
    }
}

fn sha256(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 16384];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
