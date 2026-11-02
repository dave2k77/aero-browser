//! C03: pure construction of the Bubblewrap command that launches Chromium.
//!
//! [`SandboxSpec::build`] turns already-resolved host facts into one explicit
//! argument vector. It touches no filesystem and spawns nothing; C04 resolves
//! the inputs (canonical executables, a [`HostLayout`] probe, the B06 run
//! directory), spawns the command, reads `DevToolsActivePort` and reaps.
//!
//! Policy (implementation plan §7.8):
//! - user, PID, IPC and UTS namespaces; die with parent; new session; no
//!   capabilities; the environment cleared and rebuilt from fixed values;
//! - read-only `/usr`, merged-/usr symlinks recreated (never a host bind of a
//!   symlink target outside `/usr`), and only the listed `/etc` runtime files;
//! - private `/tmp`, `/dev/shm` and `/home/aero`; the run's `profile` and
//!   `staging` directories are the only writable host mounts;
//! - Chromium's own directory read-only at `/opt/aero/browser`;
//! - DevTools on loopback with port 0 in the private profile, and no flag that
//!   weakens Chromium's own sandbox or web security.
//!
//! Not isolated, by design for now: the host network namespace is shared, so
//! the browser can reach loopback services and abstract Unix sockets (for
//! example an X server's `@/tmp/.X11-unix/X0`). Windowed X11 is a weaker
//! boundary than Wayland. Both are reported in [`SandboxSpec::isolation_limits`].

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

/// Where the browser directory is mounted inside the sandbox.
pub const SANDBOX_BROWSER_DIR: &str = "/opt/aero/browser";
/// The sandbox home directory (private tmpfs).
pub const SANDBOX_HOME: &str = "/home/aero";
/// Chromium's profile inside the sandbox (bound to the run's `profile`).
pub const SANDBOX_PROFILE: &str = "/home/aero/profile";
/// Download/staging directory inside the sandbox (bound to the run's `staging`).
pub const SANDBOX_STAGING: &str = "/home/aero/staging";

/// Top-level paths that are directories on split-/usr hosts and symlinks into
/// `/usr` on merged-/usr hosts.
pub const MERGED_USR_PATHS: &[&str] = &["/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32"];
/// Read-only runtime configuration mounted when present: dynamic linker cache,
/// time zone, name resolution, fonts and certificates.
pub const ETC_RUNTIME_PATHS: &[&str] = &[
    "/etc/ld.so.cache",
    "/etc/localtime",
    "/etc/resolv.conf",
    "/etc/hosts",
    "/etc/host.conf",
    "/etc/nsswitch.conf",
    "/etc/gai.conf",
    "/etc/fonts",
    "/etc/ssl",
    "/etc/ca-certificates",
];

/// What a host runtime path is, as observed by `lstat` (C04 probes it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostEntry {
    Directory,
    File,
    /// The link text, unresolved.
    Symlink(PathBuf),
}

/// Observed kinds of `/usr`, [`MERGED_USR_PATHS`] and [`ETC_RUNTIME_PATHS`].
/// A path absent from `entries` does not exist on the host.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HostLayout {
    pub entries: BTreeMap<PathBuf, HostEntry>,
}

impl HostLayout {
    /// `lstat` `/usr`, [`MERGED_USR_PATHS`] and [`ETC_RUNTIME_PATHS`] on this
    /// host. Paths that cannot be inspected are treated as absent.
    pub fn probe() -> Self {
        let mut entries = BTreeMap::new();
        let candidates = std::iter::once("/usr")
            .chain(MERGED_USR_PATHS.iter().copied())
            .chain(ETC_RUNTIME_PATHS.iter().copied());
        for path in candidates.map(Path::new) {
            let Ok(meta) = std::fs::symlink_metadata(path) else {
                continue;
            };
            let entry = if meta.file_type().is_symlink() {
                match std::fs::read_link(path) {
                    Ok(target) => HostEntry::Symlink(target),
                    Err(_) => continue,
                }
            } else if meta.is_dir() {
                HostEntry::Directory
            } else {
                HostEntry::File
            };
            entries.insert(path.to_owned(), entry);
        }
        Self { entries }
    }
}

/// How the browser is displayed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Display {
    /// Default: no display server is reachable through mounts or environment.
    Headless,
    /// Windowed through one Wayland socket (the host path of
    /// `$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY`); nothing else from the runtime dir.
    Wayland { socket: PathBuf },
    /// Windowed through the X11 socket `/tmp/.X11-unix/X<display>` and an
    /// optional authorization file. X11 is a weaker boundary than Wayland.
    X11 {
        display: u16,
        xauthority: Option<PathBuf>,
    },
}

/// Browser window size in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
}

/// Largest accepted viewport side.
pub const MAX_VIEWPORT_SIDE: u32 = 16_384;

/// Resolved host facts for one browser launch. Every path must be absolute
/// and normalized (C04 canonicalizes them); nothing here is looked up.
#[derive(Clone, Debug)]
pub struct SandboxRequest {
    /// Canonical Bubblewrap executable.
    pub bwrap: PathBuf,
    /// Canonical Chromium executable (not a Snap wrapper). Its directory,
    /// which holds the browser's resources, is mounted read-only.
    pub browser_executable: PathBuf,
    /// The B06 run directory; its `profile` and `staging` children are the
    /// only writable host mounts.
    pub run_directory: PathBuf,
    pub host: HostLayout,
    pub display: Display,
    pub viewport: Viewport,
    /// `LANG` inside the sandbox, e.g. `C.UTF-8`.
    pub locale: String,
}

/// A known weakness of the constructed sandbox, reported rather than hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IsolationLimit {
    /// The host network namespace is shared: loopback services and abstract
    /// Unix sockets are reachable.
    HostNetwork,
    /// An X11 display is reachable; X11 does not isolate clients.
    X11Display,
}

/// Why a launch could not be constructed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SandboxError {
    /// A path was relative, not normalized, contained NUL, or was `/`.
    InvalidPath(&'static str),
    /// The browser executable is a Snap path, not a directly mountable binary.
    SnapWrapper,
    /// Writable run directories overlap a read-only source or each other.
    OverlappingPaths,
    /// A host runtime path has an unsupported kind or symlink target.
    UnsupportedLayout(PathBuf),
    /// A required host runtime path is missing.
    MissingRuntime(PathBuf),
    InvalidDisplay,
    InvalidViewport,
    InvalidLocale,
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPath(field) => write!(f, "{field} must be an absolute normalized path"),
            Self::SnapWrapper => f.write_str("the browser executable is a Snap wrapper"),
            Self::OverlappingPaths => f.write_str("sandbox mount sources overlap"),
            Self::UnsupportedLayout(path) => {
                write!(f, "unsupported host runtime layout at {}", path.display())
            }
            Self::MissingRuntime(path) => write!(f, "missing host runtime {}", path.display()),
            Self::InvalidDisplay => f.write_str("invalid display configuration"),
            Self::InvalidViewport => f.write_str("invalid viewport"),
            Self::InvalidLocale => f.write_str("invalid locale"),
        }
    }
}

impl std::error::Error for SandboxError {}

/// A fully constructed launch: `program` with `args`. Spawn it with an empty
/// environment; the sandbox rebuilds its own.
#[derive(Clone, Debug)]
pub struct SandboxSpec {
    program: PathBuf,
    args: Vec<OsString>,
    devtools_active_port: PathBuf,
    limits: Vec<IsolationLimit>,
}

impl SandboxSpec {
    /// Construct the launch, or refuse inputs that would weaken the policy.
    pub fn build(request: &SandboxRequest) -> Result<Self, SandboxError> {
        clean_absolute(&request.bwrap, "bwrap")?;
        clean_absolute(&request.browser_executable, "browser_executable")?;
        clean_absolute(&request.run_directory, "run_directory")?;
        let executable = &request.browser_executable;
        if executable.starts_with("/snap") {
            return Err(SandboxError::SnapWrapper);
        }
        // Chromium's resources live beside its executable; never mount `/`.
        let (Some(browser_dir), Some(executable_name)) =
            (executable.parent(), executable.file_name())
        else {
            return Err(SandboxError::InvalidPath("browser_executable"));
        };
        if browser_dir == Path::new("/") {
            return Err(SandboxError::InvalidPath("browser_executable"));
        }
        let Viewport { width, height } = request.viewport;
        if !(1..=MAX_VIEWPORT_SIDE).contains(&width) || !(1..=MAX_VIEWPORT_SIDE).contains(&height) {
            return Err(SandboxError::InvalidViewport);
        }
        if !valid_locale(&request.locale) {
            return Err(SandboxError::InvalidLocale);
        }

        let mut runtime = Mounts::default();
        let usr = Path::new("/usr");
        match request.host.entries.get(usr) {
            Some(HostEntry::Directory) => runtime.read_only(usr, usr),
            Some(_) => return Err(SandboxError::UnsupportedLayout(usr.into())),
            None => return Err(SandboxError::MissingRuntime(usr.into())),
        }
        for path in MERGED_USR_PATHS.iter().map(Path::new) {
            match request.host.entries.get(path) {
                None => {}
                Some(HostEntry::Directory) => runtime.read_only(path, path),
                Some(HostEntry::Symlink(target)) if links_into_usr(target) => {
                    runtime.symlink(target, path);
                }
                Some(_) => return Err(SandboxError::UnsupportedLayout(path.into())),
            }
        }
        // A bind follows a symlinked source on the host (e.g. resolv.conf).
        for path in ETC_RUNTIME_PATHS.iter().map(Path::new) {
            if request.host.entries.contains_key(path) {
                runtime.read_only(path, path);
            }
        }

        // Display: only the one socket (and X authority), mounted after the
        // private roots they live in.
        let mut display_env: Vec<(&str, OsString)> = Vec::new();
        let mut display_mounts = Mounts::default();
        let mut limits = vec![IsolationLimit::HostNetwork];
        let headless = match &request.display {
            Display::Headless => true,
            Display::Wayland { socket } => {
                if clean_absolute(socket, "socket").is_err() {
                    return Err(SandboxError::InvalidDisplay);
                }
                display_mounts.tmpfs("/run/aero");
                display_mounts.read_only(socket, Path::new("/run/aero/wayland-0"));
                display_env.push(("XDG_RUNTIME_DIR", "/run/aero".into()));
                display_env.push(("WAYLAND_DISPLAY", "wayland-0".into()));
                false
            }
            Display::X11 {
                display,
                xauthority,
            } => {
                let socket = PathBuf::from(format!("/tmp/.X11-unix/X{display}"));
                display_mounts.read_only(&socket, &socket);
                if let Some(authority) = xauthority {
                    if clean_absolute(authority, "xauthority").is_err() {
                        return Err(SandboxError::InvalidDisplay);
                    }
                    display_mounts.read_only(authority, Path::new("/home/aero/.Xauthority"));
                    display_env.push(("XAUTHORITY", "/home/aero/.Xauthority".into()));
                }
                display_env.push(("DISPLAY", format!(":{display}").into()));
                limits.push(IsolationLimit::X11Display);
                false
            }
        };

        // The run directory is the only writable source: it must not overlap
        // anything mounted read-only (or the browser directory) either way.
        let run = &request.run_directory;
        let read_only_sources = runtime
            .read_only_sources()
            .chain(display_mounts.read_only_sources())
            .chain(std::iter::once(browser_dir));
        for source in read_only_sources {
            if run.starts_with(source) || source.starts_with(run) {
                return Err(SandboxError::OverlappingPaths);
            }
        }

        let profile = run.join("profile");
        let staging = run.join("staging");
        let mut args: Vec<OsString> = Vec::new();
        for flag in [
            "--unshare-user",
            "--unshare-pid",
            "--unshare-ipc",
            "--unshare-uts",
            "--die-with-parent",
            "--new-session",
            "--cap-drop",
            "ALL",
            "--clearenv",
        ] {
            args.push(flag.into());
        }
        let base_env: [(&str, OsString); 3] = [
            ("HOME", SANDBOX_HOME.into()),
            ("PATH", "/usr/bin:/bin".into()),
            ("LANG", request.locale.clone().into()),
        ];
        for (name, value) in base_env.into_iter().chain(display_env) {
            args.extend(["--setenv".into(), name.into(), value]);
        }
        runtime.append_to(&mut args);
        let mut private = Mounts::default();
        private.op1("--proc", "/proc");
        private.op1("--dev", "/dev");
        private.tmpfs("/dev/shm");
        private.tmpfs("/tmp");
        private.tmpfs(SANDBOX_HOME);
        private.writable(&profile, Path::new(SANDBOX_PROFILE));
        private.writable(&staging, Path::new(SANDBOX_STAGING));
        private.read_only(browser_dir, Path::new(SANDBOX_BROWSER_DIR));
        private.append_to(&mut args);
        display_mounts.append_to(&mut args);
        args.extend(["--chdir".into(), SANDBOX_HOME.into()]);

        // The sandboxed path is absolute, so Bubblewrap cannot read it as an
        // option.
        args.push(Path::new(SANDBOX_BROWSER_DIR).join(executable_name).into());
        let mut browser: Vec<String> = vec![
            "--remote-debugging-address=127.0.0.1".into(),
            "--remote-debugging-port=0".into(),
            format!("--user-data-dir={SANDBOX_PROFILE}"),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
        ];
        if headless {
            browser.push("--headless".into());
        }
        browser.push(format!("--window-size={width},{height}"));
        browser.push("about:blank".into());
        args.extend(browser.into_iter().map(OsString::from));

        Ok(Self {
            program: request.bwrap.clone(),
            args,
            devtools_active_port: profile.join("DevToolsActivePort"),
            limits,
        })
    }

    /// The Bubblewrap executable.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// Bubblewrap options, then the sandboxed browser path and its flags.
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// Host path where Chromium writes its DevTools port for this run.
    pub fn devtools_active_port(&self) -> &Path {
        &self.devtools_active_port
    }

    /// Known weaknesses of this sandbox.
    pub fn isolation_limits(&self) -> &[IsolationLimit] {
        &self.limits
    }
}

/// Bubblewrap mount operations in command-line order.
#[derive(Default)]
struct Mounts(Vec<(&'static str, Vec<OsString>)>);

impl Mounts {
    fn read_only(&mut self, source: &Path, destination: &Path) {
        self.0
            .push(("--ro-bind", vec![source.into(), destination.into()]));
    }
    fn writable(&mut self, source: &Path, destination: &Path) {
        self.0
            .push(("--bind", vec![source.into(), destination.into()]));
    }
    fn symlink(&mut self, target: &Path, path: &Path) {
        self.0.push(("--symlink", vec![target.into(), path.into()]));
    }
    fn tmpfs(&mut self, destination: &str) {
        self.op1("--tmpfs", destination);
    }
    fn op1(&mut self, option: &'static str, destination: &str) {
        self.0.push((option, vec![destination.into()]));
    }
    fn read_only_sources(&self) -> impl Iterator<Item = &Path> {
        self.0
            .iter()
            .filter(|(option, _)| *option == "--ro-bind")
            .map(|(_, operands)| Path::new(&operands[0]))
    }
    fn append_to(self, args: &mut Vec<OsString>) {
        for (option, operands) in self.0 {
            args.push(option.into());
            args.extend(operands);
        }
    }
}

/// Absolute, not `/`, no NUL, and already in normal form: no `.`/`..`
/// components, repeated or trailing separators.
fn clean_absolute(path: &Path, field: &'static str) -> Result<(), SandboxError> {
    if path.is_absolute() && path != Path::new("/") && normalized(path) {
        Ok(())
    } else {
        Err(SandboxError::InvalidPath(field))
    }
}

fn normalized(path: &Path) -> bool {
    let bytes = path.as_os_str().as_bytes();
    if bytes.is_empty() || bytes.contains(&0) {
        return false;
    }
    let rebuilt: PathBuf = path.components().collect();
    rebuilt.as_os_str() == path.as_os_str()
        && path
            .components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
}

/// A merged-/usr link such as `usr/lib` or `/usr/lib`: normalized, and
/// resolving (from `/`) to `/usr` or below without any `..`.
fn links_into_usr(target: &Path) -> bool {
    normalized(target) && Path::new("/").join(target).starts_with("/usr")
}

/// `LANG` value: 1–64 of ASCII letters, digits, `_`, `.`, `@`, `-`.
fn valid_locale(locale: &str) -> bool {
    (1..=64).contains(&locale.len())
        && locale
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'@' | b'-'))
}
