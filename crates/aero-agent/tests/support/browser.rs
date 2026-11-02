//! Sandboxed browser for scenarios (test-only): B06 private task/run/runtime
//! directories, the C03 sandbox command and the C04/C05 `BrowserRuntime`.
//!
//! Field order is drop order: the runtime (registry, connection, then the
//! pidfd-verified process) is torn down before its directories and lease.

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use aero_agent::config::{PrivateState, RuntimeDirs, StateConfig, TaskLease};
use aero_cdp::launcher::{Display, HostLayout, SandboxRequest, SandboxSpec, Viewport};
use aero_cdp::runtime::{BrowserRuntime, LaunchLimits};
use aero_core::ids::{RunId, RuntimeEpoch, TaskId};

pub struct BrowserHarness {
    runtime: Option<BrowserRuntime>,
    _runtime: RuntimeDirs,
    _lease: TaskLease,
    _state: PrivateState,
    _home: tempfile::TempDir,
}

/// A required environment path, canonicalized. Missing is an error the test
/// reports as a failure (SPEC-30), never a skip.
fn required_path(variable: &str, default: Option<&str>) -> Result<PathBuf, String> {
    let value = std::env::var_os(variable)
        .or_else(|| default.map(Into::into))
        .ok_or_else(|| format!("{variable} must name the required executable"))?;
    std::fs::canonicalize(&value)
        .map_err(|error| format!("{variable}={}: {error}", PathBuf::from(&value).display()))
}

impl BrowserHarness {
    pub async fn launch() -> Result<Self, String> {
        let chromium = required_path("AERO_CHROMIUM", None)?;
        let bwrap = required_path("AERO_BWRAP", Some("/usr/bin/bwrap"))?;
        let home = tempfile::tempdir().map_err(|e| e.to_string())?;
        std::fs::set_permissions(home.path(), std::fs::Permissions::from_mode(0o700))
            .map_err(|e| e.to_string())?;
        let state = StateConfig::resolve(Some(home.path()), None)
            .and_then(|config| config.open())
            .map_err(|e| format!("private state: {e:?}"))?;
        let lease = state
            .acquire_task(&TaskId::from("fixture-task"))
            .map_err(|e| format!("task lease: {e:?}"))?;
        let runtime = lease
            .create_runtime(&RunId::from("fixture-run"), &RuntimeEpoch::from("epoch-1"))
            .map_err(|e| format!("runtime directories: {e:?}"))?;
        let run_directory = runtime
            .profile_directory()
            .parent()
            .ok_or("runtime directory")?
            .to_owned();
        let spec = SandboxSpec::build(&SandboxRequest {
            bwrap,
            browser_executable: chromium,
            run_directory,
            host: HostLayout::probe(),
            display: Display::Headless,
            viewport: Viewport {
                width: 1280,
                height: 800,
            },
            locale: "C.UTF-8".into(),
        })
        .map_err(|e| format!("sandbox: {e}"))?;
        let browser = BrowserRuntime::start(&spec, LaunchLimits::default())
            .await
            .map_err(|e| format!("browser runtime: {e} ({e:?})"))?;
        Ok(Self {
            runtime: Some(browser),
            _runtime: runtime,
            _lease: lease,
            _state: state,
            _home: home,
        })
    }

    pub fn runtime(&self) -> &BrowserRuntime {
        self.runtime
            .as_ref()
            .expect("the runtime lives until shutdown consumes the harness")
    }

    /// Stop the runtime and require the sandbox verified empty.
    pub async fn shutdown(mut self) -> Result<(), String> {
        let Some(runtime) = self.runtime.take() else {
            return Ok(());
        };
        let report = runtime.shutdown().await;
        if report.sandbox_emptied {
            Ok(())
        } else {
            Err(format!("sandbox not verified empty: {report:?}"))
        }
    }
}
