//! C03: the Bubblewrap launch command is constructed purely and checked as
//! data. C04: the `browser` module (feature `browser-tests`) launches the
//! pinned Chromium from `AERO_CHROMIUM`; everything else starts no process.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use aero_cdp::launcher::{
    Display, HostEntry, HostLayout, IsolationLimit, SandboxError, SandboxRequest, SandboxSpec,
    Viewport,
};

const BROWSER_DIR: &str = "/home/tester/.cache/aerobrowser/browsers/156.0.8078.47/chrome-linux64";
const RUN: &str = "/home/tester/.local/state/aerobrowser/tasks/abc/runs/r1";

fn layout(entries: &[(&str, HostEntry)]) -> HostLayout {
    HostLayout {
        entries: entries
            .iter()
            .map(|(path, entry)| (PathBuf::from(path), entry.clone()))
            .collect(),
    }
}
fn link(target: &str) -> HostEntry {
    HostEntry::Symlink(target.into())
}
/// Ubuntu 24.04-like merged-/usr host.
fn merged_usr() -> HostLayout {
    layout(&[
        ("/usr", HostEntry::Directory),
        ("/bin", link("usr/bin")),
        ("/sbin", link("usr/sbin")),
        ("/lib", link("usr/lib")),
        ("/lib64", link("usr/lib64")),
        ("/etc/ld.so.cache", HostEntry::File),
        ("/etc/localtime", link("/usr/share/zoneinfo/Etc/UTC")),
        (
            "/etc/resolv.conf",
            link("../run/systemd/resolve/stub-resolv.conf"),
        ),
        ("/etc/hosts", HostEntry::File),
        ("/etc/nsswitch.conf", HostEntry::File),
        ("/etc/fonts", HostEntry::Directory),
        ("/etc/ssl", HostEntry::Directory),
        ("/etc/ca-certificates", HostEntry::Directory),
    ])
}
fn request() -> SandboxRequest {
    SandboxRequest {
        bwrap: "/usr/bin/bwrap".into(),
        browser_executable: format!("{BROWSER_DIR}/chrome").into(),
        run_directory: RUN.into(),
        host: merged_usr(),
        display: Display::Headless,
        viewport: Viewport {
            width: 1280,
            height: 800,
        },
        locale: "C.UTF-8".into(),
    }
}

/// A parsed launch: Bubblewrap operations in order, then the browser argv.
struct Parsed {
    /// (option, operands) in command-line order.
    ops: Vec<(String, Vec<String>)>,
    browser: Vec<String>,
}
fn parse(spec: &SandboxSpec) -> Parsed {
    let args: Vec<String> = spec
        .args()
        .iter()
        .map(|a| a.to_str().expect("test paths are UTF-8").to_owned())
        .collect();
    let mut ops = Vec::new();
    let mut index = 0;
    while index < args.len() && args[index].starts_with("--") {
        let option = args[index].clone();
        let operands = match option.as_str() {
            "--bind" | "--ro-bind" | "--symlink" | "--setenv" => 2,
            "--tmpfs" | "--proc" | "--dev" | "--chdir" | "--cap-drop" => 1,
            "--unshare-user" | "--unshare-pid" | "--unshare-ipc" | "--unshare-uts"
            | "--die-with-parent" | "--new-session" | "--clearenv" => 0,
            other => panic!("unexpected or unsafe bwrap option {other}"),
        };
        ops.push((option, args[index + 1..index + 1 + operands].to_vec()));
        index += 1 + operands;
    }
    Parsed {
        ops,
        browser: args[index..].to_vec(),
    }
}
impl Parsed {
    fn position(&self, option: &str, operands: &[&str]) -> Option<usize> {
        self.ops.iter().position(|(o, args)| {
            o == option && args.iter().map(String::as_str).eq(operands.iter().copied())
        })
    }
    fn has(&self, option: &str, operands: &[&str]) -> bool {
        self.position(option, operands).is_some()
    }
    fn env(&self) -> BTreeMap<String, String> {
        self.ops
            .iter()
            .filter(|(o, _)| o == "--setenv")
            .map(|(_, a)| (a[0].clone(), a[1].clone()))
            .collect()
    }
    /// (source, destination) of bind mounts with the given option.
    fn binds(&self, option: &str) -> Vec<(String, String)> {
        self.ops
            .iter()
            .filter(|(o, _)| o == option)
            .map(|(_, a)| (a[0].clone(), a[1].clone()))
            .collect()
    }
}
fn build(request: &SandboxRequest) -> Parsed {
    parse(&SandboxSpec::build(request).expect("valid request"))
}

const SANDBOX_WEAKENING_FLAGS: &[&str] = &[
    "--no-sandbox",
    "--disable-setuid-sandbox",
    "--disable-namespace-sandbox",
    "--disable-seccomp-filter-sandbox",
    "--disable-gpu-sandbox",
    "--no-zygote",
    "--single-process",
    "--disable-web-security",
    "--remote-allow-origins",
];

#[test]
fn command_policy() {
    let spec = SandboxSpec::build(&request()).expect("valid request");
    assert_eq!(spec.program(), PathBuf::from("/usr/bin/bwrap"));
    let parsed = parse(&spec);

    // Namespaces, process ownership and capabilities.
    for flag in [
        "--unshare-user",
        "--unshare-pid",
        "--unshare-ipc",
        "--unshare-uts",
        "--die-with-parent",
        "--new-session",
        "--clearenv",
    ] {
        assert!(parsed.has(flag, &[]), "missing {flag}");
    }
    assert!(parsed.has("--cap-drop", &["ALL"]));

    // Environment: cleared first, then exactly these values.
    let clear = parsed.position("--clearenv", &[]).unwrap();
    assert!(
        parsed
            .ops
            .iter()
            .enumerate()
            .all(|(i, (o, _))| o != "--setenv" || i > clear)
    );
    let expected_env: BTreeMap<String, String> = [
        ("HOME", "/home/aero"),
        ("PATH", "/usr/bin:/bin"),
        ("LANG", "C.UTF-8"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect();
    assert_eq!(parsed.env(), expected_env);

    // Private roots, in an order where later mounts land inside them.
    let dev = parsed.position("--dev", &["/dev"]).expect("private /dev");
    let shm = parsed
        .position("--tmpfs", &["/dev/shm"])
        .expect("private /dev/shm");
    assert!(dev < shm);
    assert!(parsed.has("--proc", &["/proc"]));
    assert!(parsed.has("--tmpfs", &["/tmp"]));
    let home = parsed
        .position("--tmpfs", &["/home/aero"])
        .expect("private home");
    let profile_src = format!("{RUN}/profile");
    let staging_src = format!("{RUN}/staging");
    let profile = parsed
        .position("--bind", &[&profile_src, "/home/aero/profile"])
        .expect("profile bind");
    let staging = parsed
        .position("--bind", &[&staging_src, "/home/aero/staging"])
        .expect("staging bind");
    assert!(home < profile && home < staging);
    assert!(parsed.has("--chdir", &["/home/aero"]));

    // The only writable host mounts are the run's profile and staging.
    let writable: BTreeSet<String> = parsed.binds("--bind").into_iter().map(|(s, _)| s).collect();
    assert_eq!(
        writable,
        BTreeSet::from([profile_src.clone(), staging_src.clone()])
    );

    // Read-only sources: /usr, present /etc runtime files, the browser dir.
    let read_only: BTreeSet<String> = parsed
        .binds("--ro-bind")
        .into_iter()
        .map(|(source, destination)| {
            if source != BROWSER_DIR {
                assert_eq!(source, destination, "runtime paths keep their location");
            }
            source
        })
        .collect();
    let expected_ro: BTreeSet<String> = [
        "/usr",
        "/etc/ld.so.cache",
        "/etc/localtime",
        "/etc/resolv.conf",
        "/etc/hosts",
        "/etc/nsswitch.conf",
        "/etc/fonts",
        "/etc/ssl",
        "/etc/ca-certificates",
        BROWSER_DIR,
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    assert_eq!(read_only, expected_ro);
    assert!(parsed.has("--ro-bind", &[BROWSER_DIR, "/opt/aero/browser"]));
    // Merged /usr: the links are recreated, never bound.
    for (target, path) in [
        ("usr/bin", "/bin"),
        ("usr/sbin", "/sbin"),
        ("usr/lib", "/lib"),
        ("usr/lib64", "/lib64"),
    ] {
        assert!(parsed.has("--symlink", &[target, path]), "{path}");
    }

    // No host home, project, session bus, runtime dir or secrets anywhere.
    let everything: Vec<&String> = parsed
        .ops
        .iter()
        .flat_map(|(o, a)| std::iter::once(o).chain(a))
        .chain(&parsed.browser)
        .collect();
    for arg in &everything {
        if arg.starts_with("/home/tester") {
            assert!(
                [BROWSER_DIR, profile_src.as_str(), staging_src.as_str()].contains(&arg.as_str()),
                "host home path leaked: {arg}"
            );
        }
        for forbidden in [
            "dbus",
            "DBUS",
            "/run/user",
            "XDG_RUNTIME_DIR",
            "DISPLAY",
            "API_KEY",
            "TOKEN",
        ] {
            assert!(!arg.contains(forbidden), "{forbidden} in {arg}");
        }
    }

    // Browser: loopback DevTools on a fresh port in the private profile.
    assert_eq!(parsed.browser[0], "/opt/aero/browser/chrome");
    for flag in [
        "--remote-debugging-address=127.0.0.1",
        "--remote-debugging-port=0",
        "--user-data-dir=/home/aero/profile",
        "--no-first-run",
        "--no-default-browser-check",
        "--headless",
        "--window-size=1280,800",
    ] {
        assert!(parsed.browser.iter().any(|a| a == flag), "missing {flag}");
    }
    assert_eq!(parsed.browser.last().unwrap(), "about:blank");
    let profiles: Vec<&String> = parsed
        .browser
        .iter()
        .filter(|a| a.starts_with("--user-data-dir"))
        .collect();
    assert_eq!(profiles, ["--user-data-dir=/home/aero/profile"]);
    for arg in &parsed.browser {
        for weakening in SANDBOX_WEAKENING_FLAGS {
            assert!(!arg.starts_with(weakening), "sandbox-weakening flag {arg}");
        }
    }
    assert_eq!(
        spec.devtools_active_port(),
        PathBuf::from(format!("{RUN}/profile/DevToolsActivePort"))
    );
    assert_eq!(spec.isolation_limits(), &[IsolationLimit::HostNetwork]);
}

#[test]
fn split_usr_directories_are_bound_and_missing_paths_skipped() {
    let mut r = request();
    r.host = layout(&[
        ("/usr", HostEntry::Directory),
        ("/bin", HostEntry::Directory),
        ("/lib", HostEntry::Directory),
        ("/lib64", link("/usr/lib64")),
        ("/etc/hosts", HostEntry::File),
    ]);
    let parsed = build(&r);
    assert!(parsed.has("--ro-bind", &["/bin", "/bin"]));
    assert!(parsed.has("--ro-bind", &["/lib", "/lib"]));
    assert!(parsed.has("--symlink", &["/usr/lib64", "/lib64"]));
    assert!(parsed.has("--ro-bind", &["/etc/hosts", "/etc/hosts"]));
    for absent in [
        "/sbin",
        "/lib32",
        "/libx32",
        "/etc/resolv.conf",
        "/etc/gai.conf",
    ] {
        assert!(
            parsed
                .ops
                .iter()
                .all(|(_, a)| !a.iter().any(|x| x == absent)),
            "{absent} is not on the host and must not be mounted"
        );
    }
}

#[test]
fn unsupported_host_layouts_are_refused() {
    for (path, entry) in [
        ("/lib", link("/opt/vendor/lib")),
        ("/lib", link("../opt/lib")),
        ("/lib", link("usr/../etc")),
        ("/lib", link("")),
        ("/lib", HostEntry::File),
        ("/usr", link("/opt/usr")),
        ("/usr", HostEntry::File),
    ] {
        let mut r = request();
        r.host.entries.insert(path.into(), entry.clone());
        assert_eq!(
            SandboxSpec::build(&r).err(),
            Some(SandboxError::UnsupportedLayout(path.into())),
            "{path} as {entry:?}"
        );
    }
    let mut r = request();
    r.host.entries.remove(&PathBuf::from("/usr"));
    assert_eq!(
        SandboxSpec::build(&r).err(),
        Some(SandboxError::MissingRuntime("/usr".into()))
    );
}

#[test]
fn unsafe_paths_are_refused() {
    type Edit = fn(&mut SandboxRequest);
    let cases: &[(&str, Edit, SandboxError)] = &[
        (
            "relative bwrap",
            |r| r.bwrap = "bwrap".into(),
            SandboxError::InvalidPath("bwrap"),
        ),
        (
            "relative browser",
            |r| r.browser_executable = "chrome".into(),
            SandboxError::InvalidPath("browser_executable"),
        ),
        (
            "dot-dot browser",
            |r| r.browser_executable = format!("{BROWSER_DIR}/../chrome").into(),
            SandboxError::InvalidPath("browser_executable"),
        ),
        (
            "dot browser",
            |r| r.browser_executable = format!("{BROWSER_DIR}/./chrome").into(),
            SandboxError::InvalidPath("browser_executable"),
        ),
        (
            "trailing slash",
            |r| r.run_directory = format!("{RUN}/").into(),
            SandboxError::InvalidPath("run_directory"),
        ),
        (
            "NUL in run dir",
            |r| r.run_directory = format!("{RUN}\0x").into(),
            SandboxError::InvalidPath("run_directory"),
        ),
        (
            "root run dir",
            |r| r.run_directory = "/".into(),
            SandboxError::InvalidPath("run_directory"),
        ),
        (
            "browser in root dir",
            |r| r.browser_executable = "/chrome".into(),
            SandboxError::InvalidPath("browser_executable"),
        ),
        (
            "snap browser",
            |r| {
                r.browser_executable =
                    "/snap/chromium/current/usr/lib/chromium-browser/chrome".into()
            },
            SandboxError::SnapWrapper,
        ),
        (
            "run dir inside browser dir",
            |r| r.run_directory = format!("{BROWSER_DIR}/run").into(),
            SandboxError::OverlappingPaths,
        ),
        (
            "run dir contains browser dir",
            |r| r.run_directory = "/home/tester/.cache".into(),
            SandboxError::OverlappingPaths,
        ),
        (
            "run dir is /etc",
            |r| r.run_directory = "/etc".into(),
            SandboxError::OverlappingPaths,
        ),
        (
            "run dir inside /usr",
            |r| r.run_directory = "/usr/local/run".into(),
            SandboxError::OverlappingPaths,
        ),
        (
            "zero width",
            |r| r.viewport.width = 0,
            SandboxError::InvalidViewport,
        ),
        (
            "huge height",
            |r| r.viewport.height = 16_385,
            SandboxError::InvalidViewport,
        ),
        (
            "empty locale",
            |r| r.locale = String::new(),
            SandboxError::InvalidLocale,
        ),
        (
            "locale with path",
            |r| r.locale = "../../x".into(),
            SandboxError::InvalidLocale,
        ),
        (
            "locale with space",
            |r| r.locale = "C UTF-8".into(),
            SandboxError::InvalidLocale,
        ),
    ];
    for (name, edit, expected) in cases {
        let mut r = request();
        edit(&mut r);
        assert_eq!(
            SandboxSpec::build(&r).err().as_ref(),
            Some(expected),
            "{name}"
        );
    }
}

#[test]
fn display_modes_expose_only_their_socket() {
    // Wayland: one socket in a private runtime dir, no X11, not headless.
    let mut r = request();
    r.display = Display::Wayland {
        socket: "/run/user/1000/wayland-0".into(),
    };
    let spec = SandboxSpec::build(&r).unwrap();
    let parsed = parse(&spec);
    assert!(parsed.has("--tmpfs", &["/run/aero"]));
    assert!(parsed.has(
        "--ro-bind",
        &["/run/user/1000/wayland-0", "/run/aero/wayland-0"]
    ));
    assert_eq!(
        parsed.env().get("XDG_RUNTIME_DIR").map(String::as_str),
        Some("/run/aero")
    );
    assert_eq!(
        parsed.env().get("WAYLAND_DISPLAY").map(String::as_str),
        Some("wayland-0")
    );
    assert!(!parsed.env().contains_key("DISPLAY"));
    assert!(!parsed.browser.iter().any(|a| a == "--headless"));
    assert!(
        parsed
            .ops
            .iter()
            .all(|(_, a)| !a.iter().any(|x| x == "/run/user/1000")),
        "the whole runtime directory must not be mounted"
    );
    assert_eq!(spec.isolation_limits(), &[IsolationLimit::HostNetwork]);

    // X11: the display socket and authorization file only, flagged as weaker.
    let mut r = request();
    r.display = Display::X11 {
        display: 1,
        xauthority: Some("/home/tester/.Xauthority".into()),
    };
    let spec = SandboxSpec::build(&r).unwrap();
    let parsed = parse(&spec);
    let tmp = parsed.position("--tmpfs", &["/tmp"]).unwrap();
    let socket = parsed
        .position("--ro-bind", &["/tmp/.X11-unix/X1", "/tmp/.X11-unix/X1"])
        .expect("X11 socket");
    assert!(
        tmp < socket,
        "the socket must be mounted inside the private /tmp"
    );
    let home = parsed.position("--tmpfs", &["/home/aero"]).unwrap();
    let auth = parsed
        .position(
            "--ro-bind",
            &["/home/tester/.Xauthority", "/home/aero/.Xauthority"],
        )
        .expect("X authority");
    assert!(home < auth);
    assert_eq!(parsed.env().get("DISPLAY").map(String::as_str), Some(":1"));
    assert_eq!(
        parsed.env().get("XAUTHORITY").map(String::as_str),
        Some("/home/aero/.Xauthority")
    );
    assert!(!parsed.env().contains_key("WAYLAND_DISPLAY"));
    assert!(!parsed.browser.iter().any(|a| a == "--headless"));
    assert_eq!(
        spec.isolation_limits(),
        &[IsolationLimit::HostNetwork, IsolationLimit::X11Display]
    );

    // Display paths must be safe too.
    for display in [
        Display::Wayland {
            socket: "wayland-0".into(),
        },
        Display::Wayland {
            socket: "/run/user/1000/../wayland-0".into(),
        },
        Display::Wayland {
            socket: format!("{RUN}/wayland-0").into(),
        },
        Display::X11 {
            display: 0,
            xauthority: Some(".Xauthority".into()),
        },
        Display::X11 {
            display: 0,
            xauthority: Some(format!("{RUN}/profile/x").into()),
        },
    ] {
        let mut r = request();
        r.display = display.clone();
        assert!(
            matches!(
                SandboxSpec::build(&r),
                Err(SandboxError::InvalidDisplay | SandboxError::OverlappingPaths)
            ),
            "{display:?}"
        );
    }
}

// ---- C04: host probe and launch preconditions (no process started) ---------

#[test]
fn host_probe_reports_this_hosts_runtime() {
    let probed = HostLayout::probe();
    assert_eq!(
        probed.entries.get(&PathBuf::from("/usr")),
        Some(&HostEntry::Directory)
    );
    // Independent check of every candidate path against lstat.
    let candidates = std::iter::once("/usr")
        .chain(aero_cdp::launcher::MERGED_USR_PATHS.iter().copied())
        .chain(aero_cdp::launcher::ETC_RUNTIME_PATHS.iter().copied());
    for path in candidates {
        let expected = std::fs::symlink_metadata(path).ok().map(|meta| {
            if meta.file_type().is_symlink() {
                HostEntry::Symlink(std::fs::read_link(path).unwrap())
            } else if meta.is_dir() {
                HostEntry::Directory
            } else {
                HostEntry::File
            }
        });
        assert_eq!(
            probed.entries.get(&PathBuf::from(path)),
            expected.as_ref(),
            "{path}"
        );
    }
    assert_eq!(
        probed.entries.len(),
        probed
            .entries
            .keys()
            .filter(|k| std::fs::symlink_metadata(k).is_ok())
            .count()
    );
}

#[tokio::test]
async fn stale_devtools_port_file_is_refused_before_spawning() {
    use aero_cdp::runtime::{BrowserProcess, LaunchError, LaunchLimits};
    let run = tempfile::tempdir().unwrap();
    for child in ["profile", "staging"] {
        std::fs::create_dir(run.path().join(child)).unwrap();
    }
    let stale = run.path().join("profile/DevToolsActivePort");
    std::fs::write(&stale, b"1\n/devtools/browser/old").unwrap();
    let mut r = request();
    // A Bubblewrap that does not exist: reaching spawn would report Spawn.
    r.bwrap = "/nonexistent/bwrap".into();
    r.browser_executable = "/usr/bin/true".into();
    r.run_directory = run.path().to_owned();
    let spec = SandboxSpec::build(&r).unwrap();
    let result = BrowserProcess::launch(&spec, LaunchLimits::default()).await;
    assert_eq!(result.err(), Some(LaunchError::StaleProfile));
    assert_eq!(
        std::fs::read(&stale).unwrap(),
        b"1\n/devtools/browser/old",
        "the stale file is reported, not deleted"
    );
}

// ---- C04: real browser launch (feature `browser-tests`) --------------------

#[cfg(feature = "browser-tests")]
mod browser {
    use super::*;
    use aero_cdp::CdpConnection;
    use aero_cdp::TransportLimits;
    use aero_cdp::runtime::{BrowserProcess, LaunchError, LaunchLimits};
    use aero_core::ids::CdpSessionId;
    use serde_json::{Value, json};
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    const CALL: Duration = Duration::from_secs(10);
    /// Browser tests share one host process table; run them one at a time.
    static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// The pinned browser. With this feature enabled a missing browser is a
    /// failure, never a skip (implementation plan §6, SPEC-30).
    fn chromium() -> PathBuf {
        std::env::var_os("AERO_CHROMIUM")
            .map(PathBuf::from)
            .and_then(|path| std::fs::canonicalize(path).ok())
            .expect("browser-tests requires AERO_CHROMIUM to name the pinned Chromium executable")
    }

    struct Run {
        directory: tempfile::TempDir,
        spec: SandboxSpec,
    }

    fn prepare(browser: &std::path::Path) -> Run {
        let directory = tempfile::tempdir().unwrap();
        for child in ["profile", "staging"] {
            let path = directory.path().join(child);
            std::fs::create_dir(&path).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let bwrap = std::env::var_os("AERO_BWRAP").unwrap_or_else(|| "/usr/bin/bwrap".into());
        let spec = SandboxSpec::build(&SandboxRequest {
            bwrap: std::fs::canonicalize(bwrap).expect("Bubblewrap is installed"),
            browser_executable: browser.to_owned(),
            run_directory: directory.path().to_owned(),
            host: HostLayout::probe(),
            display: Display::Headless,
            viewport: Viewport {
                width: 1280,
                height: 800,
            },
            locale: "C.UTF-8".into(),
        })
        .unwrap();
        Run { directory, spec }
    }

    /// Host PIDs of processes executing from the sandbox browser directory.
    fn sandbox_processes() -> Vec<(u32, String)> {
        let mut found = Vec::new();
        for entry in std::fs::read_dir("/proc").unwrap().flatten() {
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else {
                continue;
            };
            let command = String::from_utf8_lossy(&raw).replace('\0', " ");
            if command.starts_with("/opt/aero/browser/") {
                found.push((pid, command));
            }
        }
        found
    }

    fn status_field(pid: u32, field: &str) -> Option<String> {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        status
            .lines()
            .find_map(|line| line.strip_prefix(field)?.strip_prefix(':'))
            .map(|value| value.trim().to_owned())
    }

    async fn shutdown(browser: BrowserProcess) -> aero_cdp::runtime::ShutdownReport {
        tokio::task::spawn_blocking(move || browser.shutdown())
            .await
            .unwrap()
    }

    /// Open `url` in a new page and return its text once `ready` accepts it
    /// (or the last text seen after ten seconds).
    async fn page_text(connection: &CdpConnection, url: &str, ready: fn(&str) -> bool) -> String {
        let target = connection
            .call("Target.createTarget", json!({ "url": url }), None, CALL)
            .await
            .unwrap();
        let attached = connection
            .call(
                "Target.attachToTarget",
                json!({ "targetId": target["targetId"], "flatten": true }),
                None,
                CALL,
            )
            .await
            .unwrap();
        let session = CdpSessionId::from(attached["sessionId"].as_str().unwrap());
        let mut last = String::new();
        for _ in 0..100 {
            let reply = connection
                .call(
                    "Runtime.evaluate",
                    json!({
                        "expression": "document.readyState === 'complete' && document.body ? document.body.innerText : ''",
                        "returnByValue": true
                    }),
                    Some(&session),
                    CALL,
                )
                .await;
            if let Ok(Value::String(text)) = reply.map(|r| r["result"]["value"].clone()) {
                if ready(&text) {
                    return text;
                }
                last = text;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        last
    }

    #[tokio::test]
    async fn launch_reaches_cdp() {
        let chrome = chromium();
        let _serial = SERIAL.lock().await;
        let run = prepare(&chrome);
        let browser = BrowserProcess::launch(&run.spec, LaunchLimits::default())
            .await
            .expect("browser becomes ready");
        assert!(browser.endpoint().starts_with("ws://127.0.0.1:"));
        assert!(browser.endpoint().contains("/devtools/browser/"));
        assert!(run.spec.devtools_active_port().is_file());
        let connection = CdpConnection::connect(browser.endpoint(), TransportLimits::default())
            .await
            .unwrap();
        let version = connection
            .call("Browser.getVersion", json!({}), None, CALL)
            .await
            .unwrap();
        assert!(
            version["product"].as_str().unwrap().contains("Chrome/156."),
            "{version}"
        );
        assert!(!sandbox_processes().is_empty());
        connection.close().await;

        let report = shutdown(browser).await;
        assert!(report.sandbox_emptied, "{report:?}");
        assert!(report.status.is_some());
        assert_eq!(sandbox_processes(), vec![], "no sandbox process survives");
        drop(run.directory);
    }

    #[tokio::test]
    async fn readiness_failure_reaps_child() {
        let chrome = chromium();
        let _serial = SERIAL.lock().await;

        // Deadline too short for Chromium to write its port file.
        let run = prepare(&chrome);
        let limits = LaunchLimits {
            readiness_timeout: Duration::from_millis(1),
            ..LaunchLimits::default()
        };
        let result = BrowserProcess::launch(&run.spec, limits).await;
        assert!(
            matches!(result, Err(LaunchError::ReadinessTimeout { .. })),
            "{:?}",
            result.err()
        );
        assert_eq!(sandbox_processes(), vec![], "timed-out sandbox was reaped");

        // A "browser" that exits at once reports its status and stderr.
        let early = prepare(std::path::Path::new("/usr/bin/sleep"));
        match BrowserProcess::launch(&early.spec, LaunchLimits::default()).await {
            Err(LaunchError::EarlyExit { code, stderr, .. }) => {
                assert_eq!(code, Some(1));
                assert!(stderr.contains("sleep"), "{stderr}");
            }
            other => panic!("expected an early exit, got {:?}", other.err()),
        }
        assert_eq!(sandbox_processes(), vec![]);
    }

    #[tokio::test]
    async fn sandbox_cannot_read_host_canary() {
        let chrome = chromium();
        let _serial = SERIAL.lock().await;
        let nonce = format!(
            "{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        // Synthetic canary outside every mount (not a real secret).
        let outside = tempfile::tempdir().unwrap();
        let canary = outside.path().join("canary.txt");
        std::fs::write(&canary, format!("aero-canary-{nonce}")).unwrap();
        let run = prepare(&chrome);
        // Positive control: a file in the run's staging mount is readable.
        std::fs::write(
            run.directory.path().join("staging/control.txt"),
            format!("aero-control-{nonce}"),
        )
        .unwrap();
        let browser = BrowserProcess::launch(&run.spec, LaunchLimits::default())
            .await
            .unwrap();
        let connection = CdpConnection::connect(browser.endpoint(), TransportLimits::default())
            .await
            .unwrap();

        let control = page_text(&connection, "file:///home/aero/staging/control.txt", |t| {
            t.contains("aero-control-")
        })
        .await;
        assert!(
            control.contains(&format!("aero-control-{nonce}")),
            "{control}"
        );
        let leaked = page_text(&connection, &format!("file://{}", canary.display()), |t| {
            !t.is_empty()
        })
        .await;
        assert!(!leaked.is_empty(), "the canary navigation produced no page");
        assert!(!leaked.contains("aero-canary-"), "host canary was readable");
        let listing = page_text(
            &connection,
            &format!("file://{}/", outside.path().display()),
            |t| !t.is_empty(),
        )
        .await;
        assert!(
            !listing.contains("canary.txt"),
            "host directory was listable"
        );

        connection.close().await;
        assert!(shutdown(browser).await.sandbox_emptied);
    }

    #[tokio::test]
    async fn renderer_sandbox_remains_enabled() {
        let chrome = chromium();
        let _serial = SERIAL.lock().await;
        let run = prepare(&chrome);
        let browser = BrowserProcess::launch(&run.spec, LaunchLimits::default())
            .await
            .unwrap();
        let connection = CdpConnection::connect(browser.endpoint(), TransportLimits::default())
            .await
            .unwrap();
        // Chromium's own report.
        let report = page_text(&connection, "chrome://sandbox", |t| t.contains("sandboxed")).await;
        assert!(report.contains("You are adequately sandboxed"), "{report}");

        // Independent evidence from the host: every renderer is seccomp-
        // filtered and in a PID namespace nested below the browser's.
        let processes = sandbox_processes();
        let browser_ns = processes
            .iter()
            .find(|(_, command)| command.contains("--remote-debugging-port=0"))
            .map(|(pid, _)| std::fs::read_link(format!("/proc/{pid}/ns/pid")).unwrap())
            .expect("main browser process");
        let renderers: Vec<u32> = processes
            .iter()
            .filter(|(_, command)| command.contains("--type=renderer"))
            .map(|(pid, _)| *pid)
            .collect();
        assert!(!renderers.is_empty(), "{processes:?}");
        for pid in renderers {
            assert_eq!(
                status_field(pid, "Seccomp").as_deref(),
                Some("2"),
                "renderer {pid}"
            );
            assert_ne!(
                std::fs::read_link(format!("/proc/{pid}/ns/pid")).unwrap(),
                browser_ns,
                "renderer {pid} shares the browser's PID namespace"
            );
        }
        connection.close().await;
        assert!(shutdown(browser).await.sandbox_emptied);
    }
}
