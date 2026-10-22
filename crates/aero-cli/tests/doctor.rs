use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

fn doctor(browser: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_aero"))
        .args(["doctor", "--json", "--chromium"])
        .arg(browser)
        .env("AERO_BWRAP", "")
        .env("SYNTHETIC_CHILD_ENV_SENTINEL", "test-only-secret")
        .output()
        .expect("run doctor")
}

fn executable(directory: &Path, body: &str) -> std::path::PathBuf {
    let path = directory.join("fixture-browser");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn doctor_missing_browser_is_nonzero() {
    let output = doctor(Path::new(""));
    assert_eq!(output.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&output.stdout).expect("doctor emits JSON");
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["browser"]["available"], false);
    assert_eq!(report["runtime_ready"], false);
    assert!(output.stderr.is_empty());
}

#[test]
fn doctor_reports_configured_executable_without_claiming_sandbox() {
    let directory = tempfile::tempdir().unwrap();
    let browser = executable(
        directory.path(),
        "[ -z \"${SYNTHETIC_CHILD_ENV_SENTINEL:-}\" ] || exit 99; printf 'Chromium 123.0.0.0\\n'",
    );
    let output = doctor(&browser);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["browser"]["available"], true);
    assert_eq!(report["browser"]["version"], "Chromium 123.0.0.0");
    assert_eq!(
        report["browser"]["path"],
        browser.canonicalize().unwrap().to_str().unwrap()
    );
    assert_eq!(report["browser_lock"]["status"], "mismatch");
    assert_eq!(report["sandbox"]["status"], "not_checked");
    assert_eq!(report["keyring"]["status"], "not_checked");
    assert_eq!(report["runtime_ready"], false);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn doctor_does_not_fall_back_from_explicit_missing_path() {
    let directory = tempfile::tempdir().unwrap();
    let fallback = executable(directory.path(), "printf 'Chromium 123.0.0.0\\n'");
    let output = Command::new(env!("CARGO_BIN_EXE_aero"))
        .args(["doctor", "--json", "--chromium"])
        .arg(directory.path().join("does-not-exist"))
        .env("AERO_CHROMIUM", fallback)
        .env("AERO_BWRAP", "")
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["browser"]["available"], false);
    assert_eq!(report["browser"]["error"], "not_found_or_not_executable");
}

#[test]
fn doctor_version_probe_times_out() {
    let directory = tempfile::tempdir().unwrap();
    let browser = executable(directory.path(), "while :; do :; done");
    let start = Instant::now();
    let output = doctor(&browser);
    assert!(start.elapsed() < Duration::from_secs(5));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["browser"]["available"], false);
    assert_eq!(report["browser"]["error"], "version_timeout");
}

#[test]
fn doctor_failed_version_hides_untrusted_output() {
    let directory = tempfile::tempdir().unwrap();
    let browser = executable(
        directory.path(),
        "printf 'SYNTHETIC_SECRET_SENTINEL' >&2; exit 1",
    );
    let output = doctor(&browser);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["browser"]["available"], false);
    assert_eq!(report["browser"]["error"], "version_command_failed");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SYNTHETIC_SECRET_SENTINEL"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("SYNTHETIC_SECRET_SENTINEL"));
}

#[test]
fn doctor_rejects_oversized_version_output() {
    let directory = tempfile::tempdir().unwrap();
    let browser = executable(
        directory.path(),
        "i=0; while [ \"$i\" -lt 5000 ]; do printf x; i=$((i+1)); done",
    );
    let output = doctor(&browser);
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["browser"]["available"], false);
    assert_eq!(report["browser"]["error"], "version_output_too_large");
}

#[test]
fn help_and_version_do_not_probe_a_browser() {
    let directory = tempfile::tempdir().unwrap();
    let browser = executable(directory.path(), "exit 99");
    for flag in ["--help", "--version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_aero"))
            .arg(flag)
            .env("AERO_CHROMIUM", &browser)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        assert!(String::from_utf8_lossy(&output.stdout).contains("aero"));
    }
}

#[test]
fn invalid_arguments_exit_two() {
    let output = Command::new(env!("CARGO_BIN_EXE_aero"))
        .arg("--definitely-invalid")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unexpected argument"));
}
